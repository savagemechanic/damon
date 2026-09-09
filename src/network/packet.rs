use super::{
    address::{IpAddress, Ipv4Address, Ipv6Address, MacAddress},
    InterfaceId,
};
use std::{fmt, ops::Range};

pub const ETHERNET_IPV4: u16 = 0x0800;
pub const ETHERNET_ARP: u16 = 0x0806;
pub const ETHERNET_IPV6: u16 = 0x86dd;
pub const IP_ICMP: u8 = 1;
pub const IP_TCP: u8 = 6;
pub const IP_UDP: u8 = 17;
pub const IPV6_ICMP: u8 = 58;
const MAX_LAYERS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ethernet {
    pub source: MacAddress,
    pub destination: MacAddress,
    pub ether_type: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arp {
    pub operation: u16,
    pub sender_mac: MacAddress,
    pub sender_ip: Ipv4Address,
    pub target_mac: MacAddress,
    pub target_ip: Ipv4Address,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ip {
    pub source: IpAddress,
    pub destination: IpAddress,
    pub next_protocol: u8,
    pub hop_limit: u8,
    pub fragmented: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tcp {
    pub source_port: u16,
    pub destination_port: u16,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Udp {
    pub source_port: u16,
    pub destination_port: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icmp {
    pub kind: u8,
    pub code: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Ethernet(Ethernet),
    Arp(Arp),
    Ip(Ip),
    Tcp(Tcp),
    Udp(Udp),
    Icmp(Icmp),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    bytes: Vec<u8>,
    pub timestamp_micros: u64,
    pub interface: InterfaceId,
    pub direction: Direction,
    pub layers: Vec<Layer>,
    pub payload: Range<usize>,
}

impl Packet {
    pub fn parse_ethernet(
        bytes: Vec<u8>,
        timestamp_micros: u64,
        interface: InterfaceId,
        direction: Direction,
    ) -> Result<Self, PacketError> {
        if bytes.len() < 14 {
            return Err(PacketError::Truncated("Ethernet header"));
        }
        let ethernet = Ethernet {
            destination: mac(&bytes[0..6]),
            source: mac(&bytes[6..12]),
            ether_type: be16(&bytes, 12)?,
        };
        let mut packet = Self {
            payload: 14..bytes.len(),
            bytes,
            timestamp_micros,
            interface,
            direction,
            layers: vec![Layer::Ethernet(ethernet)],
        };
        match ethernet.ether_type {
            ETHERNET_ARP => packet.parse_arp(14)?,
            ETHERNET_IPV4 => packet.parse_ipv4(14)?,
            ETHERNET_IPV6 => packet.parse_ipv6(14)?,
            _ => {}
        }
        Ok(packet)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn payload_bytes(&self) -> &[u8] {
        &self.bytes[self.payload.clone()]
    }

    fn push(&mut self, layer: Layer) -> Result<(), PacketError> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(PacketError::TooManyLayers);
        }
        self.layers.push(layer);
        Ok(())
    }

    fn parse_arp(&mut self, offset: usize) -> Result<(), PacketError> {
        let bytes = self
            .bytes
            .get(offset..)
            .ok_or(PacketError::Truncated("ARP"))?;
        if bytes.len() < 28 {
            return Err(PacketError::Truncated("ARP packet"));
        }
        if be16(bytes, 0)? != 1
            || be16(bytes, 2)? != ETHERNET_IPV4
            || bytes[4] != 6
            || bytes[5] != 4
        {
            return Err(PacketError::Malformed("unsupported ARP address format"));
        }
        let arp = Arp {
            operation: be16(bytes, 6)?,
            sender_mac: mac(&bytes[8..14]),
            sender_ip: ipv4(&bytes[14..18]),
            target_mac: mac(&bytes[18..24]),
            target_ip: ipv4(&bytes[24..28]),
        };
        self.payload = offset + 28..offset + 28;
        self.push(Layer::Arp(arp))
    }

    fn parse_ipv4(&mut self, offset: usize) -> Result<(), PacketError> {
        let bytes = self
            .bytes
            .get(offset..)
            .ok_or(PacketError::Truncated("IPv4"))?;
        if bytes.len() < 20 {
            return Err(PacketError::Truncated("IPv4 header"));
        }
        if bytes[0] >> 4 != 4 {
            return Err(PacketError::Malformed("invalid IPv4 version"));
        }
        let header_len = usize::from(bytes[0] & 0x0f) * 4;
        if header_len < 20 || header_len > bytes.len() {
            return Err(PacketError::Malformed("invalid IPv4 header length"));
        }
        let total_len = usize::from(be16(bytes, 2)?);
        if total_len < header_len || total_len > bytes.len() {
            return Err(PacketError::Truncated("IPv4 payload"));
        }
        let fragment = be16(bytes, 6)?;
        let ip = Ip {
            source: IpAddress::V4(ipv4(&bytes[12..16])),
            destination: IpAddress::V4(ipv4(&bytes[16..20])),
            next_protocol: bytes[9],
            hop_limit: bytes[8],
            fragmented: fragment & 0x3fff != 0,
        };
        self.push(Layer::Ip(ip))?;
        let payload = offset + header_len..offset + total_len;
        self.payload = payload.clone();
        // Only an initial, unfragmented datagram has a complete transport header.
        if fragment & 0x3fff == 0 {
            self.parse_transport(ip.next_protocol, payload)?;
        }
        Ok(())
    }

    fn parse_ipv6(&mut self, offset: usize) -> Result<(), PacketError> {
        let bytes = self
            .bytes
            .get(offset..)
            .ok_or(PacketError::Truncated("IPv6"))?;
        if bytes.len() < 40 {
            return Err(PacketError::Truncated("IPv6 header"));
        }
        if bytes[0] >> 4 != 6 {
            return Err(PacketError::Malformed("invalid IPv6 version"));
        }
        let payload_len = usize::from(be16(bytes, 4)?);
        if payload_len > bytes.len() - 40 {
            return Err(PacketError::Truncated("IPv6 payload"));
        }
        let ip = Ip {
            source: IpAddress::V6(ipv6(&bytes[8..24])),
            destination: IpAddress::V6(ipv6(&bytes[24..40])),
            next_protocol: bytes[6],
            hop_limit: bytes[7],
            fragmented: false,
        };
        self.push(Layer::Ip(ip))?;
        let payload = offset + 40..offset + 40 + payload_len;
        self.payload = payload.clone();
        // Extension-header traversal is deliberately deferred. Unknown next
        // headers remain a valid IP observation rather than a guessed parse.
        self.parse_transport(ip.next_protocol, payload)
    }

    fn parse_transport(&mut self, protocol: u8, range: Range<usize>) -> Result<(), PacketError> {
        let bytes = &self.bytes[range.clone()];
        match protocol {
            IP_TCP => {
                if bytes.len() < 20 {
                    return Err(PacketError::Truncated("TCP header"));
                }
                let header_len = usize::from(bytes[12] >> 4) * 4;
                if header_len < 20 || header_len > bytes.len() {
                    return Err(PacketError::Malformed("invalid TCP header length"));
                }
                let tcp = Tcp {
                    source_port: be16(bytes, 0)?,
                    destination_port: be16(bytes, 2)?,
                    flags: be16(bytes, 12)? & 0x01ff,
                };
                self.payload = range.start + header_len..range.end;
                self.push(Layer::Tcp(tcp))
            }
            IP_UDP => {
                if bytes.len() < 8 {
                    return Err(PacketError::Truncated("UDP header"));
                }
                let length = usize::from(be16(bytes, 4)?);
                if length < 8 || length > bytes.len() {
                    return Err(PacketError::Malformed("invalid UDP length"));
                }
                let udp = Udp {
                    source_port: be16(bytes, 0)?,
                    destination_port: be16(bytes, 2)?,
                };
                self.payload = range.start + 8..range.start + length;
                self.push(Layer::Udp(udp))
            }
            IP_ICMP | IPV6_ICMP => {
                if bytes.len() < 4 {
                    return Err(PacketError::Truncated("ICMP header"));
                }
                let icmp = Icmp {
                    kind: bytes[0],
                    code: bytes[1],
                };
                self.payload = range.start + 4..range.end;
                self.push(Layer::Icmp(icmp))
            }
            _ => Ok(()),
        }
    }
}

pub trait Protocol {
    fn recognize(&self, bytes: &[u8]) -> bool;
    fn parse(&self, bytes: &[u8]) -> Result<Layer, PacketError>;
    fn encode(&self, layer: Layer, output: &mut Vec<u8>) -> Result<(), PacketError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketError {
    Truncated(&'static str),
    Malformed(&'static str),
    TooManyLayers,
    WrongLayer,
}

impl fmt::Display for PacketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(layer) => write!(formatter, "truncated {layer}"),
            Self::Malformed(reason) => formatter.write_str(reason),
            Self::TooManyLayers => formatter.write_str("packet layer limit exceeded"),
            Self::WrongLayer => formatter.write_str("protocol cannot encode this layer"),
        }
    }
}

impl std::error::Error for PacketError {}

fn be16(bytes: &[u8], offset: usize) -> Result<u16, PacketError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(PacketError::Truncated("field"))?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn mac(bytes: &[u8]) -> MacAddress {
    MacAddress::new(bytes.try_into().expect("validated MAC slice"))
}

fn ipv4(bytes: &[u8]) -> Ipv4Address {
    Ipv4Address::new(bytes.try_into().expect("validated IPv4 slice"))
}

fn ipv6(bytes: &[u8]) -> Ipv6Address {
    Ipv6Address::new(bytes.try_into().expect("validated IPv6 slice"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ethernet_ipv4_udp() -> Vec<u8> {
        let mut bytes = vec![
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00,
            0x45, 0, 0, 32, 0, 1, 0, 0, 64, 17, 0, 0, 192, 168, 1, 2, 8, 8, 8, 8, 0x14, 0xe9, 0,
            53, 0, 12, 0, 0,
        ];
        bytes.extend_from_slice(b"test");
        bytes
    }

    #[test]
    fn progressively_parses_without_copying_payload() {
        let packet =
            Packet::parse_ethernet(ethernet_ipv4_udp(), 7, InterfaceId(4), Direction::Outbound)
                .unwrap();
        assert_eq!(packet.layers.len(), 3);
        assert!(matches!(packet.layers[1], Layer::Ip(_)));
        assert_eq!(
            packet.layers[2],
            Layer::Udp(Udp {
                source_port: 5353,
                destination_port: 53
            })
        );
        assert_eq!(packet.payload_bytes(), b"test");
        assert_eq!(packet.bytes().len(), 46);
    }

    #[test]
    fn every_truncation_fails_without_panicking() {
        let bytes = ethernet_ipv4_udp();
        for end in 0..bytes.len() {
            assert!(
                Packet::parse_ethernet(
                    bytes[..end].to_vec(),
                    0,
                    InterfaceId(0),
                    Direction::Unknown
                )
                .is_err(),
                "accepted prefix of {end} bytes"
            );
        }
    }

    #[test]
    fn malformed_lengths_are_rejected() {
        let mut bytes = ethernet_ipv4_udp();
        bytes[16] = 0xff;
        bytes[17] = 0xff;
        assert!(matches!(
            Packet::parse_ethernet(bytes, 0, InterfaceId(1), Direction::Unknown),
            Err(PacketError::Truncated("IPv4 payload"))
        ));
    }
}
