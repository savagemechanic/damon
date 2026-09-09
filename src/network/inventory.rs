use super::address::IpAddress;
#[cfg(any(test, target_os = "linux"))]
use super::address::{Ipv4Address, Ipv6Address};
use std::io;
#[cfg(target_os = "linux")]
use std::{collections::HashMap, fs};
#[cfg(target_os = "macos")]
use std::{path::Path, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub address: Option<IpAddress>,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketRecord {
    pub transport: Transport,
    pub local: Endpoint,
    pub remote: Option<Endpoint>,
    pub state: String,
    pub process_id: Option<u32>,
    pub process_name: Option<String>,
    pub inode: Option<u64>,
}

#[cfg(target_os = "linux")]
pub fn discover() -> io::Result<Vec<SocketRecord>> {
    let mut records = Vec::new();
    for (path, transport, ipv6) in [
        ("/proc/net/tcp", Transport::Tcp, false),
        ("/proc/net/tcp6", Transport::Tcp, true),
        ("/proc/net/udp", Transport::Udp, false),
        ("/proc/net/udp6", Transport::Udp, true),
    ] {
        if let Ok(text) = fs::read_to_string(path) {
            records.extend(parse_linux(&text, transport, ipv6)?);
        }
    }
    attach_linux_owners(&mut records);
    records.sort_by_key(|record| (record.local.port, record.process_id, record.inode));
    records.truncate(8192);
    Ok(records)
}

#[cfg(target_os = "macos")]
pub fn discover() -> io::Result<Vec<SocketRecord>> {
    let result = crate::process::run(
        Path::new("/"),
        "/usr/sbin/lsof",
        &["-nP", "-iTCP", "-iUDP", "-FpcPnT"],
        None,
        Duration::from_secs(10),
    );
    if !result.success {
        return Err(io::Error::other(format!(
            "socket inventory failed: {}",
            result.stderr.trim()
        )));
    }
    Ok(parse_lsof(&result.stdout))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn discover() -> io::Result<Vec<SocketRecord>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "socket inventory supports macOS and Linux",
    ))
}

#[cfg(any(test, target_os = "linux"))]
fn parse_linux(text: &str, transport: Transport, ipv6: bool) -> io::Result<Vec<SocketRecord>> {
    let mut records = Vec::new();
    for line in text.lines().skip(1).take(8192) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let local = parse_linux_endpoint(fields[1], ipv6)?;
        let remote = parse_linux_endpoint(fields[2], ipv6)?;
        let state = linux_state(fields[3], transport).to_string();
        let inode = fields[9].parse().ok();
        records.push(SocketRecord {
            transport,
            local,
            remote: (remote.port != 0 || remote.address.is_some()).then_some(remote),
            state,
            process_id: None,
            process_name: None,
            inode,
        });
    }
    Ok(records)
}

#[cfg(any(test, target_os = "linux"))]
fn parse_linux_endpoint(value: &str, ipv6: bool) -> io::Result<Endpoint> {
    let (address, port) = value
        .split_once(':')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid socket endpoint"))?;
    let port = u16::from_str_radix(port, 16)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid socket port"))?;
    let address = if ipv6 {
        if address.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid IPv6 socket address",
            ));
        }
        let mut bytes = [0_u8; 16];
        let (chunks, _) = address.as_bytes().as_chunks::<8>();
        let (outputs, _) = bytes.as_chunks_mut::<4>();
        for (chunk, output) in chunks.iter().zip(outputs.iter_mut()) {
            let text = std::str::from_utf8(chunk)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid IPv6 hex"))?;
            output.copy_from_slice(
                &u32::from_str_radix(text, 16)
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid IPv6 socket address")
                    })?
                    .to_le_bytes(),
            );
        }
        (bytes != [0; 16]).then_some(IpAddress::V6(Ipv6Address::new(bytes)))
    } else {
        let raw = u32::from_str_radix(address, 16).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid IPv4 socket address")
        })?;
        (raw != 0).then_some(IpAddress::V4(Ipv4Address::new(raw.to_le_bytes())))
    };
    Ok(Endpoint { address, port })
}

#[cfg(any(test, target_os = "linux"))]
fn linux_state(value: &str, transport: Transport) -> &'static str {
    if transport == Transport::Udp {
        return "UNCONNECTED";
    }
    match value {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}

#[cfg(target_os = "linux")]
fn attach_linux_owners(records: &mut [SocketRecord]) {
    let mut by_inode = HashMap::<u64, Vec<usize>>::new();
    for (index, record) in records.iter().enumerate() {
        if let Some(inode) = record.inode {
            by_inode.entry(inode).or_default().push(index);
        }
    }
    let Ok(processes) = fs::read_dir("/proc") else {
        return;
    };
    for process in processes.flatten().take(4096) {
        let Ok(pid) = process.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let name = fs::read_to_string(process.path().join("comm"))
            .ok()
            .map(|name| name.trim().chars().take(128).collect::<String>());
        let Ok(descriptors) = fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        for descriptor in descriptors.flatten().take(1024) {
            let Ok(target) = fs::read_link(descriptor.path()) else {
                continue;
            };
            let text = target.to_string_lossy();
            let Some(inode) = text
                .strip_prefix("socket:[")
                .and_then(|value| value.strip_suffix(']'))
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            for index in by_inode.get(&inode).into_iter().flatten() {
                records[*index].process_id = Some(pid);
                records[*index].process_name.clone_from(&name);
            }
        }
    }
}

#[cfg(any(test, target_os = "macos"))]
fn parse_lsof(text: &str) -> Vec<SocketRecord> {
    let mut records = Vec::new();
    let mut pid = None;
    let mut process = None;
    let mut transport = None;
    let mut state = String::new();
    for line in text.lines().take(32768) {
        let Some((tag, value)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => pid = value.parse().ok(),
            "c" => process = Some(value.chars().take(128).collect()),
            "P" => {
                transport = match value {
                    "TCP" => Some(Transport::Tcp),
                    "UDP" => Some(Transport::Udp),
                    _ => None,
                }
            }
            "T" if value.starts_with("ST=") => state = value[3..].to_string(),
            "n" => {
                let Some(transport) = transport else { continue };
                if let Some((local, remote)) = parse_lsof_name(value) {
                    records.push(SocketRecord {
                        transport,
                        local,
                        remote,
                        state: state.clone(),
                        process_id: pid,
                        process_name: process.clone(),
                        inode: None,
                    });
                }
            }
            _ => {}
        }
    }
    records.truncate(8192);
    records
}

#[cfg(any(test, target_os = "macos"))]
fn parse_lsof_name(value: &str) -> Option<(Endpoint, Option<Endpoint>)> {
    let (local, remote) = value
        .split_once("->")
        .map_or((value, None), |(local, remote)| (local, Some(remote)));
    Some((
        parse_display_endpoint(local)?,
        remote.and_then(parse_display_endpoint),
    ))
}

#[cfg(any(test, target_os = "macos"))]
fn parse_display_endpoint(value: &str) -> Option<Endpoint> {
    let (address, port) = value.rsplit_once(':')?;
    let address = address.trim_matches(['[', ']']);
    Some(Endpoint {
        address: (!matches!(address, "*" | "0.0.0.0" | "::"))
            .then(|| address.parse().ok())
            .flatten(),
        port: port.parse().ok()?,
    })
}

pub fn render_endpoint(endpoint: &Endpoint) -> String {
    endpoint.address.map_or_else(
        || format!("*:{}", endpoint.port),
        |address| format!("{address}:{}", endpoint.port),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_socket_rows_and_byte_order() {
        let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n   0: 0100007F:0BB8 00000000:0000 0A 0:0 00:0 0 1000 0 42\n";
        let rows = parse_linux(text, Transport::Tcp, false).unwrap();
        assert_eq!(rows[0].local.address.unwrap().to_string(), "127.0.0.1");
        assert_eq!(rows[0].local.port, 3000);
        assert_eq!(rows[0].state, "LISTEN");
    }

    #[test]
    fn parses_macos_field_output_without_column_guessing() {
        let rows = parse_lsof("p42\ncdamon\nPTCP\nTST=LISTEN\nn127.0.0.1:3000\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].process_id, Some(42));
        assert_eq!(rows[0].local.port, 3000);
        assert_eq!(rows[0].process_name.as_deref(), Some("damon"));
    }
}
