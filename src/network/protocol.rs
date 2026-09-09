//! Small protocol interpreters over fundamental network primitives.
use super::address::{IpAddress, Ipv4Address, Ipv6Address};
use std::{io, net::ToSocketAddrs};

/// Protocol implementations recognize and parse bytes; execution and policy
/// remain outside the parser.
pub trait Protocol {
    type Output;

    fn recognize(bytes: &[u8]) -> bool;
    fn parse(bytes: &[u8]) -> io::Result<Self::Output>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsAnswer {
    pub name: String,
    pub addresses: Vec<IpAddress>,
}

/// Use the operating system resolver while retaining typed addresses. DNS wire
/// parsing can be layered beneath this without changing callers.
pub fn resolve_system(name: &str) -> io::Result<DnsAnswer> {
    if name.is_empty()
        || name.len() > 253
        || name
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid DNS name",
        ));
    }
    let mut addresses = (name, 0)
        .to_socket_addrs()?
        .map(|address| match address.ip() {
            std::net::IpAddr::V4(address) => IpAddress::V4(Ipv4Address::new(address.octets())),
            std::net::IpAddr::V6(address) => IpAddress::V6(Ipv6Address::new(address.octets())),
        })
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "DNS returned no addresses",
        ));
    }
    Ok(DnsAnswer {
        name: name.to_ascii_lowercase(),
        addresses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_resolution_uses_local_fixture_without_public_network() {
        let answer = resolve_system("localhost").unwrap();
        assert!(!answer.addresses.is_empty());
        assert!(resolve_system("bad name").is_err());
    }
}
