use super::{address::Cidr, address::IpAddress, InterfaceId};
#[cfg(target_os = "linux")]
use std::fs;
use std::io;
#[cfg(target_os = "macos")]
use std::{path::Path, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub destination: Cidr,
    pub gateway: Option<IpAddress>,
    pub interface: InterfaceId,
    pub metric: Option<u32>,
    pub flags: u32,
}

pub const ROUTE_UP: u32 = 1;

pub fn select(routes: &[Route], destination: IpAddress) -> Option<&Route> {
    routes
        .iter()
        .filter(|route| route.flags & ROUTE_UP != 0 && route.destination.contains(destination))
        .max_by_key(|route| {
            (
                route.destination.prefix(),
                std::cmp::Reverse(route.metric.unwrap_or(u32::MAX)),
            )
        })
}

pub fn is_local(routes: &[Route], destination: IpAddress) -> bool {
    select(routes, destination).is_some_and(|route| route.gateway.is_none())
}

pub fn default_gateway(routes: &[Route]) -> Option<&Route> {
    routes
        .iter()
        .filter(|route| {
            route.flags & ROUTE_UP != 0
                && route.destination.prefix() == 0
                && route.gateway.is_some()
        })
        .min_by_key(|route| route.metric.unwrap_or(u32::MAX))
}

#[cfg(target_os = "linux")]
pub fn discover() -> io::Result<Vec<Route>> {
    parse_linux_ipv4(&fs::read_to_string("/proc/net/route")?)
}

#[cfg(target_os = "macos")]
pub fn discover() -> io::Result<Vec<Route>> {
    let result = crate::process::run(
        Path::new("/"),
        "/usr/sbin/netstat",
        &["-rn", "-f", "inet"],
        None,
        Duration::from_secs(5),
    );
    if !result.success {
        return Err(io::Error::other(format!(
            "route discovery failed: {}",
            result.stderr.trim()
        )));
    }
    parse_macos_ipv4(&result.stdout)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn discover() -> io::Result<Vec<Route>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "route discovery supports macOS and Linux",
    ))
}

#[cfg(target_os = "linux")]
fn parse_linux_ipv4(text: &str) -> io::Result<Vec<Route>> {
    let mut routes = Vec::new();
    for line in text.lines().skip(1).take(4096) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 8 {
            continue;
        }
        let value = |field: &str| {
            u32::from_str_radix(field, 16)
                .map(u32::swap_bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid route hex"))
        };
        let destination = value(fields[1])?;
        let gateway = value(fields[2])?;
        let flags = u32::from_str_radix(fields[3], 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid route flags"))?;
        let metric = fields[6].parse().ok();
        let mask = value(fields[7])?;
        let prefix = mask.count_ones() as u8;
        if mask
            != (if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            })
        {
            continue;
        }
        let Some(interface) = super::interface::index_for_name(fields[0]) else {
            continue;
        };
        routes.push(Route {
            destination: Cidr::new(
                IpAddress::V4(super::address::Ipv4Address(destination)),
                prefix,
            )
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            gateway: (gateway != 0).then_some(IpAddress::V4(super::address::Ipv4Address(gateway))),
            interface,
            metric,
            flags: if flags & 1 != 0 { ROUTE_UP } else { 0 },
        });
    }
    Ok(routes)
}

#[cfg(target_os = "macos")]
fn parse_macos_ipv4(text: &str) -> io::Result<Vec<Route>> {
    let mut routes = Vec::new();
    for line in text.lines().take(4096) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 4 || fields[0] == "Destination" || fields[0].contains(':') {
            continue;
        }
        let Some(destination) = parse_macos_destination(fields[0]) else {
            continue;
        };
        let Some(interface) = fields
            .iter()
            .rev()
            .find_map(|name| super::interface::index_for_name(name))
        else {
            continue;
        };
        let gateway = fields[1].parse::<IpAddress>().ok();
        let flags_text = fields[2];
        routes.push(Route {
            destination,
            gateway,
            interface,
            metric: None,
            flags: if flags_text.contains('U') {
                ROUTE_UP
            } else {
                0
            },
        });
    }
    Ok(routes)
}

#[cfg(target_os = "macos")]
fn parse_macos_destination(value: &str) -> Option<Cidr> {
    if value == "default" {
        return "0.0.0.0/0".parse().ok();
    }
    let (address, explicit_prefix) = value
        .split_once('/')
        .map_or((value, None), |(address, prefix)| {
            (address, prefix.parse::<u8>().ok())
        });
    let octets = address.split('.').collect::<Vec<_>>();
    if octets.is_empty() || octets.len() > 4 {
        return None;
    }
    let prefix = explicit_prefix.unwrap_or((octets.len() * 8) as u8);
    let mut padded = octets.join(".");
    for _ in octets.len()..4 {
        padded.push_str(".0");
    }
    Cidr::new(padded.parse().ok()?, prefix).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_then_lowest_metric_wins() {
        let routes = [
            Route {
                destination: "0.0.0.0/0".parse().unwrap(),
                gateway: Some("192.168.1.1".parse().unwrap()),
                interface: InterfaceId(4),
                metric: Some(20),
                flags: ROUTE_UP,
            },
            Route {
                destination: "10.0.0.0/8".parse().unwrap(),
                gateway: None,
                interface: InterfaceId(7),
                metric: Some(100),
                flags: ROUTE_UP,
            },
            Route {
                destination: "10.2.0.0/16".parse().unwrap(),
                gateway: Some("10.0.0.2".parse().unwrap()),
                interface: InterfaceId(8),
                metric: Some(30),
                flags: ROUTE_UP,
            },
        ];
        let chosen = select(&routes, "10.2.3.4".parse().unwrap()).unwrap();
        assert_eq!(chosen.interface, InterfaceId(8));
        assert!(is_local(&routes, "10.9.8.7".parse().unwrap()));
        assert!(!is_local(&routes, "8.8.8.8".parse().unwrap()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_kernel_ipv4_rows_without_human_output() {
        let text = "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\n\
                    lo 00000000 0102A8C0 0003 0 0 42 00000000 0 0 0\n\
                    lo 0000000A 00000000 0001 0 0 0 000000FF 0 0 0\n";
        let routes = parse_linux_ipv4(text).unwrap();
        assert_eq!(routes.len(), 2);
        let route = default_gateway(&routes).unwrap();
        assert_eq!(route.gateway, Some("192.168.2.1".parse().unwrap()));
        assert_eq!(route.metric, Some(42));
        assert_eq!(routes[1].destination, "10.0.0.0/8".parse().unwrap());
    }
}
