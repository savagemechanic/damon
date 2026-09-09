//! Passive neighbor observations from operating-system tables. Absence is never
//! interpreted as proof that a host does not exist.
use super::{address::IpAddress, address::MacAddress, InterfaceId};
#[cfg(target_os = "linux")]
use std::fs;
use std::{io, time::SystemTime};
#[cfg(target_os = "macos")]
use std::{path::Path, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeighborState {
    Reachable,
    Stale,
    Delay,
    Probe,
    Incomplete,
    Failed,
    Permanent,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    SystemNeighborTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Neighbor {
    pub address: IpAddress,
    pub mac: Option<MacAddress>,
    pub interface: Option<InterfaceId>,
    pub state: NeighborState,
    pub evidence: Evidence,
    pub observed_at_ms: u64,
}

pub fn discover() -> io::Result<Vec<Neighbor>> {
    discover_at(now_ms())
}

#[cfg(target_os = "linux")]
fn discover_at(observed_at_ms: u64) -> io::Result<Vec<Neighbor>> {
    let text = fs::read_to_string("/proc/net/arp")?;
    parse_linux(&text, observed_at_ms, super::interface::index_for_name)
}

#[cfg(target_os = "macos")]
fn discover_at(observed_at_ms: u64) -> io::Result<Vec<Neighbor>> {
    let arp = bounded_command("/usr/sbin/arp", &["-an"])?;
    let ndp = bounded_command("/usr/sbin/ndp", &["-an"])?;
    let mut neighbors = parse_macos_arp(&arp, observed_at_ms);
    neighbors.extend(parse_macos_ndp(&ndp, observed_at_ms));
    neighbors.sort_by_key(|neighbor| neighbor.address);
    neighbors.dedup_by_key(|neighbor| (neighbor.address, neighbor.interface));
    Ok(neighbors)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn discover_at(_observed_at_ms: u64) -> io::Result<Vec<Neighbor>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "neighbor discovery supports macOS and Linux",
    ))
}

#[cfg(target_os = "macos")]
fn bounded_command(program: &str, args: &[&str]) -> io::Result<String> {
    let result = crate::process::run(Path::new("/"), program, args, None, Duration::from_secs(5));
    if result.success {
        Ok(result.stdout)
    } else {
        Err(io::Error::other(format!(
            "neighbor table command failed: {}",
            result.stderr.trim()
        )))
    }
}

#[cfg(target_os = "linux")]
fn parse_linux(
    text: &str,
    observed_at_ms: u64,
    interface_id: impl Fn(&str) -> Option<InterfaceId>,
) -> io::Result<Vec<Neighbor>> {
    let mut neighbors = Vec::new();
    for line in text.lines().skip(1).take(4096) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            continue;
        }
        let address = fields[0]
            .parse::<IpAddress>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let flags = u32::from_str_radix(fields[2].trim_start_matches("0x"), 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid ARP flags"))?;
        let mac = if fields[3] == "00:00:00:00:00:00" {
            None
        } else {
            Some(
                fields[3]
                    .parse()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            )
        };
        neighbors.push(Neighbor {
            address,
            mac,
            interface: interface_id(fields[5]),
            state: if flags & 0x2 != 0 {
                NeighborState::Reachable
            } else {
                NeighborState::Incomplete
            },
            evidence: Evidence::SystemNeighborTable,
            observed_at_ms,
        });
    }
    neighbors.sort_by_key(|neighbor| neighbor.address);
    Ok(neighbors)
}

#[cfg(target_os = "macos")]
fn parse_macos_arp(text: &str, observed_at_ms: u64) -> Vec<Neighbor> {
    text.lines()
        .take(4096)
        .filter_map(|line| {
            let address = line.split_once('(')?.1.split_once(')')?.0.parse().ok()?;
            let after_at = line.split_once(" at ")?.1;
            let (mac, after_mac) = after_at.split_once(" on ")?;
            let interface_name = after_mac.split_whitespace().next()?;
            Some(Neighbor {
                address,
                mac: mac.parse().ok(),
                interface: super::interface::index_for_name(interface_name),
                state: if mac == "(incomplete)" {
                    NeighborState::Incomplete
                } else {
                    NeighborState::Reachable
                },
                evidence: Evidence::SystemNeighborTable,
                observed_at_ms,
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_macos_ndp(text: &str, observed_at_ms: u64) -> Vec<Neighbor> {
    text.lines()
        .skip(1)
        .take(4096)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 {
                return None;
            }
            let address = fields[0].split('%').next()?.parse().ok()?;
            let state = match fields.get(4).copied().unwrap_or("") {
                "R" => NeighborState::Reachable,
                "S" => NeighborState::Stale,
                "D" => NeighborState::Delay,
                "P" => NeighborState::Probe,
                "I" => NeighborState::Incomplete,
                _ => NeighborState::Unknown,
            };
            Some(Neighbor {
                address,
                mac: fields[1].parse().ok(),
                interface: super::interface::index_for_name(fields[2]),
                state,
                evidence: Evidence::SystemNeighborTable,
                observed_at_ms,
            })
        })
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_passive_arp_rows_and_keeps_incomplete_observations() {
        let text = "IP address HW type Flags HW address Mask Device\n\
                    192.168.1.1 0x1 0x2 aa:bb:cc:dd:ee:ff * en0\n\
                    192.168.1.9 0x1 0x0 00:00:00:00:00:00 * en0\n";
        let neighbors = parse_linux(text, 42, |_| Some(InterfaceId(7))).unwrap();
        assert_eq!(neighbors.len(), 2);
        assert_eq!(neighbors[0].state, NeighborState::Reachable);
        assert_eq!(neighbors[0].mac, Some("aa:bb:cc:dd:ee:ff".parse().unwrap()));
        assert_eq!(neighbors[1].state, NeighborState::Incomplete);
        assert_eq!(neighbors[1].mac, None);
        assert_eq!(neighbors[1].observed_at_ms, 42);
    }
}
