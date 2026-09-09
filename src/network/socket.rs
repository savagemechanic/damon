use super::SocketId;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket},
    thread,
    time::{Duration, Instant},
};

const MAX_IO: usize = 1024 * 1024;
const MAX_TIMEOUT: Duration = Duration::from_secs(120);

pub struct TcpSocket {
    pub id: SocketId,
    stream: TcpStream,
}

impl TcpSocket {
    pub fn connect(
        id: SocketId,
        address: impl ToSocketAddrs,
        timeout: Duration,
    ) -> io::Result<Self> {
        validate_timeout(timeout)?;
        let addresses = address.to_socket_addrs()?.take(32).collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "destination resolved to no addresses",
            ));
        }
        let started = Instant::now();
        let mut last = None;
        for address in addresses {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            match TcpStream::connect_timeout(&address, remaining) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(timeout))?;
                    stream.set_write_timeout(Some(timeout))?;
                    return Ok(Self { id, stream });
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "connect timed out")))
    }

    pub fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_IO {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "send exceeds 1 MiB operation limit",
            ));
        }
        self.stream.write_all(bytes)
    }

    pub fn receive(&mut self, limit: usize) -> io::Result<Vec<u8>> {
        if limit == 0 || limit > MAX_IO {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "receive limit must be 1..=1 MiB",
            ));
        }
        let mut output = vec![0; limit];
        let length = self.stream.read(&mut output)?;
        output.truncate(length);
        Ok(output)
    }

    pub fn local_address(&self) -> io::Result<SocketAddr> {
        self.stream.local_addr()
    }

    pub fn remote_address(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    pub fn close(self) -> io::Result<()> {
        match self.stream.shutdown(std::net::Shutdown::Both) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotConnected => Ok(()),
            Err(error) => Err(error),
        }
    }
}

pub struct TcpServer {
    pub id: SocketId,
    listener: TcpListener,
}

impl TcpServer {
    pub fn bind(id: SocketId, address: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self { id, listener })
    }

    pub fn local_address(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn accept(&self, id: SocketId, timeout: Duration) -> io::Result<TcpSocket> {
        validate_timeout(timeout)?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(timeout))?;
                    stream.set_write_timeout(Some(timeout))?;
                    return Ok(TcpSocket { id, stream });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "accept timed out"));
                    }
                    thread::sleep(remaining.min(Duration::from_millis(5)));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub struct DatagramSocket {
    pub id: SocketId,
    socket: UdpSocket,
}

impl DatagramSocket {
    pub fn bind(id: SocketId, address: impl ToSocketAddrs, timeout: Duration) -> io::Result<Self> {
        validate_timeout(timeout)?;
        let socket = UdpSocket::bind(address)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        Ok(Self { id, socket })
    }

    pub fn local_address(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn send_to(&self, bytes: &[u8], address: impl ToSocketAddrs) -> io::Result<usize> {
        if bytes.len() > MAX_IO {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "datagram exceeds 1 MiB operation limit",
            ));
        }
        self.socket.send_to(bytes, address)
    }

    pub fn receive_from(&self, limit: usize) -> io::Result<(Vec<u8>, SocketAddr)> {
        if limit == 0 || limit > MAX_IO {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "receive limit must be 1..=1 MiB",
            ));
        }
        let mut bytes = vec![0; limit];
        let (length, address) = self.socket.recv_from(&mut bytes)?;
        bytes.truncate(length);
        Ok((bytes, address))
    }
}

fn validate_timeout(timeout: Duration) -> io::Result<()> {
    if timeout.is_zero() || timeout > MAX_TIMEOUT {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "timeout must be greater than zero and at most 120 seconds",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_loopback_connect_accept_send_receive_and_close() {
        let server = TcpServer::bind(SocketId(1), "127.0.0.1:0").unwrap();
        let address = server.local_address().unwrap();
        let worker = thread::spawn(move || {
            let mut socket = server.accept(SocketId(2), Duration::from_secs(2)).unwrap();
            let received = socket.receive(32).unwrap();
            assert_eq!(received, b"hello");
            socket.send(b"world").unwrap();
        });
        let mut client = TcpSocket::connect(SocketId(3), address, Duration::from_secs(2)).unwrap();
        client.send(b"hello").unwrap();
        client.stream.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(client.receive(32).unwrap(), b"world");
        client.close().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn udp_loopback_is_bounded_and_reports_timeout() {
        let receiver =
            DatagramSocket::bind(SocketId(1), "127.0.0.1:0", Duration::from_millis(30)).unwrap();
        let sender =
            DatagramSocket::bind(SocketId(2), "127.0.0.1:0", Duration::from_secs(1)).unwrap();
        sender
            .send_to(b"datagram", receiver.local_address().unwrap())
            .unwrap();
        assert_eq!(receiver.receive_from(32).unwrap().0, b"datagram");
        assert!(matches!(
            receiver.receive_from(32).unwrap_err().kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
    }
}
