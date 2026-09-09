use super::{
    address::IpAddress,
    packet::{Direction, Layer, Packet, Tcp},
    ConnectionId,
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Transport {
    Tcp,
    Udp,
    Icmp,
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Observed,
    Opening,
    Established,
    Closing,
    Closed,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolGuess {
    Dns,
    Http,
    Tls,
    Ssh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Endpoint {
    address: IpAddress,
    port: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FlowKey {
    first: Endpoint,
    second: Endpoint,
    transport: Transport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub id: ConnectionId,
    pub source: IpAddress,
    pub destination: IpAddress,
    pub transport: Transport,
    pub source_port: u16,
    pub destination_port: u16,
    pub start_micros: u64,
    pub last_seen_micros: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_unknown: u64,
    pub state: ConnectionState,
    pub protocol_guess: Option<ProtocolGuess>,
}

#[derive(Clone, Debug, Default)]
pub struct ConnectionTable {
    pub connections: Vec<Connection>,
    index: HashMap<FlowKey, usize>,
}

impl ConnectionTable {
    pub fn observe(&mut self, packet: &Packet) -> Option<ConnectionId> {
        let (source, destination, transport, source_port, destination_port, tcp) =
            endpoints(packet)?;
        let source_endpoint = Endpoint {
            address: source,
            port: source_port,
        };
        let destination_endpoint = Endpoint {
            address: destination,
            port: destination_port,
        };
        let (first, second) = if source_endpoint <= destination_endpoint {
            (source_endpoint, destination_endpoint)
        } else {
            (destination_endpoint, source_endpoint)
        };
        let key = FlowKey {
            first,
            second,
            transport,
        };
        let index = if let Some(index) = self.index.get(&key).copied() {
            index
        } else {
            let index = self.connections.len();
            self.connections.push(Connection {
                id: ConnectionId(index as u32),
                source,
                destination,
                transport,
                source_port,
                destination_port,
                start_micros: packet.timestamp_micros,
                last_seen_micros: packet.timestamp_micros,
                bytes_sent: 0,
                bytes_received: 0,
                packets_sent: 0,
                packets_received: 0,
                packets_unknown: 0,
                state: ConnectionState::Observed,
                protocol_guess: guess(source_port, destination_port, transport),
            });
            self.index.insert(key, index);
            index
        };
        let connection = &mut self.connections[index];
        connection.last_seen_micros = connection.last_seen_micros.max(packet.timestamp_micros);
        let length = packet.bytes().len() as u64;
        match packet.direction {
            Direction::Outbound => {
                connection.bytes_sent = connection.bytes_sent.saturating_add(length);
                connection.packets_sent = connection.packets_sent.saturating_add(1);
            }
            Direction::Inbound => {
                connection.bytes_received = connection.bytes_received.saturating_add(length);
                connection.packets_received = connection.packets_received.saturating_add(1);
            }
            Direction::Unknown => {
                connection.packets_unknown = connection.packets_unknown.saturating_add(1);
            }
        }
        if let Some(tcp) = tcp {
            connection.state = tcp_state(connection.state, tcp.flags);
        }
        Some(connection.id)
    }
}

fn endpoints(packet: &Packet) -> Option<(IpAddress, IpAddress, Transport, u16, u16, Option<Tcp>)> {
    let ip = packet.layers.iter().find_map(|layer| match layer {
        Layer::Ip(ip) => Some(*ip),
        _ => None,
    })?;
    let (transport, source_port, destination_port, tcp) = packet
        .layers
        .iter()
        .find_map(|layer| match layer {
            Layer::Tcp(tcp) => Some((
                Transport::Tcp,
                tcp.source_port,
                tcp.destination_port,
                Some(*tcp),
            )),
            Layer::Udp(udp) => Some((Transport::Udp, udp.source_port, udp.destination_port, None)),
            Layer::Icmp(_) => Some((Transport::Icmp, 0, 0, None)),
            _ => None,
        })
        .unwrap_or((Transport::Other(ip.next_protocol), 0, 0, None));
    Some((
        ip.source,
        ip.destination,
        transport,
        source_port,
        destination_port,
        tcp,
    ))
}

fn guess(source_port: u16, destination_port: u16, transport: Transport) -> Option<ProtocolGuess> {
    let matches = |port| source_port == port || destination_port == port;
    if matches(53) {
        Some(ProtocolGuess::Dns)
    } else if transport == Transport::Tcp && matches(80) {
        Some(ProtocolGuess::Http)
    } else if transport == Transport::Tcp && matches(443) {
        Some(ProtocolGuess::Tls)
    } else if transport == Transport::Tcp && matches(22) {
        Some(ProtocolGuess::Ssh)
    } else {
        None
    }
}

fn tcp_state(previous: ConnectionState, flags: u16) -> ConnectionState {
    const FIN: u16 = 0x001;
    const SYN: u16 = 0x002;
    const RST: u16 = 0x004;
    const ACK: u16 = 0x010;
    if flags & RST != 0 {
        ConnectionState::Reset
    } else if flags & FIN != 0 {
        ConnectionState::Closing
    } else if flags & SYN != 0 && flags & ACK == 0 {
        ConnectionState::Opening
    } else if flags & ACK != 0
        && matches!(
            previous,
            ConnectionState::Opening | ConnectionState::Observed
        )
    {
        ConnectionState::Established
    } else {
        previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{packet::Direction, InterfaceId};

    fn tcp_packet(direction: Direction, flags: u8, timestamp: u64) -> Packet {
        let mut bytes = vec![
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x08, 0x00, 0x45, 0, 0, 40, 0, 1, 0, 0, 64, 6, 0,
            0, 10, 0, 0, 2, 10, 0, 0, 3, 0xc3, 0x50, 0x01, 0xbb, 0, 0, 0, 0, 0, 0, 0, 0, 0x50,
            flags, 0, 0, 0, 0, 0, 0,
        ];
        // TCP flags are the low byte of the offset/flags pair.
        bytes[47] = flags;
        Packet::parse_ethernet(bytes, timestamp, InterfaceId(2), direction).unwrap()
    }

    #[test]
    fn packets_aggregate_into_bidirectional_connection_state() {
        let mut table = ConnectionTable::default();
        let first = tcp_packet(Direction::Outbound, 0x02, 10);
        let id = table.observe(&first).unwrap();
        assert_eq!(
            table.connections[id.0 as usize].state,
            ConnectionState::Opening
        );

        let second = tcp_packet(Direction::Inbound, 0x12, 20);
        assert_eq!(table.observe(&second), Some(id));
        let connection = &table.connections[id.0 as usize];
        assert_eq!(connection.state, ConnectionState::Established);
        assert_eq!(connection.packets_sent, 1);
        assert_eq!(connection.packets_received, 1);
        assert_eq!(connection.protocol_guess, Some(ProtocolGuess::Tls));
    }
}
