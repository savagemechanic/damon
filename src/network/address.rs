use std::{fmt, net, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddress(u64);

impl MacAddress {
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(
            ((bytes[0] as u64) << 40)
                | ((bytes[1] as u64) << 32)
                | ((bytes[2] as u64) << 24)
                | ((bytes[3] as u64) << 16)
                | ((bytes[4] as u64) << 8)
                | bytes[5] as u64,
        )
    }

    pub const fn octets(self) -> [u8; 6] {
        let bytes = self.0.to_be_bytes();
        [bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]
    }

    pub const fn is_broadcast(self) -> bool {
        self.0 == 0x0000_ffff_ffff_ffff
    }

    pub const fn is_multicast(self) -> bool {
        self.octets()[0] & 1 == 1
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.octets();
        write!(
            formatter,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        )
    }
}

impl FromStr for MacAddress {
    type Err = AddressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let fields = value.split(':').collect::<Vec<_>>();
        if fields.len() != 6 {
            return Err(AddressError("MAC address needs six octets"));
        }
        let mut bytes = [0; 6];
        for (index, field) in fields.iter().enumerate() {
            if field.len() != 2 {
                return Err(AddressError("MAC octets need two hex digits"));
            }
            bytes[index] =
                u8::from_str_radix(field, 16).map_err(|_| AddressError("invalid MAC address"))?;
        }
        Ok(Self::new(bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv4Address(pub u32);

impl Ipv4Address {
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(u32::from_be_bytes(bytes))
    }

    pub const fn octets(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub const fn is_multicast(self) -> bool {
        self.0 & 0xf000_0000 == 0xe000_0000
    }

    pub const fn is_limited_broadcast(self) -> bool {
        self.0 == u32::MAX
    }
}

impl fmt::Display for Ipv4Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        net::Ipv4Addr::from(self.octets()).fmt(formatter)
    }
}

impl FromStr for Ipv4Address {
    type Err = AddressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<net::Ipv4Addr>()
            .map(|address| Self::new(address.octets()))
            .map_err(|_| AddressError("invalid IPv4 address"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv6Address(pub u128);

impl Ipv6Address {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(u128::from_be_bytes(bytes))
    }

    pub const fn octets(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }

    pub const fn is_multicast(self) -> bool {
        self.0 >> 120 == 0xff
    }
}

impl fmt::Display for Ipv6Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        net::Ipv6Addr::from(self.octets()).fmt(formatter)
    }
}

impl FromStr for Ipv6Address {
    type Err = AddressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<net::Ipv6Addr>()
            .map(|address| Self::new(address.octets()))
            .map_err(|_| AddressError("invalid IPv6 address"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IpAddress {
    V4(Ipv4Address),
    V6(Ipv6Address),
}

impl IpAddress {
    pub const fn bit_width(self) -> u8 {
        match self {
            Self::V4(_) => 32,
            Self::V6(_) => 128,
        }
    }

    pub const fn is_multicast(self) -> bool {
        match self {
            Self::V4(address) => address.is_multicast(),
            Self::V6(address) => address.is_multicast(),
        }
    }
}

impl fmt::Display for IpAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V4(address) => address.fmt(formatter),
            Self::V6(address) => address.fmt(formatter),
        }
    }
}

impl FromStr for IpAddress {
    type Err = AddressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<net::IpAddr>()
            .map(|address| match address {
                net::IpAddr::V4(address) => Self::V4(Ipv4Address::new(address.octets())),
                net::IpAddr::V6(address) => Self::V6(Ipv6Address::new(address.octets())),
            })
            .map_err(|_| AddressError("invalid IP address"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cidr {
    network: IpAddress,
    prefix: u8,
}

impl Cidr {
    pub fn new(address: IpAddress, prefix: u8) -> Result<Self, AddressError> {
        if prefix > address.bit_width() {
            return Err(AddressError("CIDR prefix is too large"));
        }
        let network = match address {
            IpAddress::V4(address) => {
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                IpAddress::V4(Ipv4Address(address.0 & mask))
            }
            IpAddress::V6(address) => {
                let mask = if prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - prefix)
                };
                IpAddress::V6(Ipv6Address(address.0 & mask))
            }
        };
        Ok(Self { network, prefix })
    }

    pub const fn network(self) -> IpAddress {
        self.network
    }

    pub const fn prefix(self) -> u8 {
        self.prefix
    }

    pub fn contains(self, address: IpAddress) -> bool {
        Self::new(address, self.prefix).is_ok_and(|candidate| candidate.network == self.network)
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.network, self.prefix)
    }
}

impl FromStr for Cidr {
    type Err = AddressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (address, prefix) = value
            .split_once('/')
            .ok_or(AddressError("CIDR needs a prefix"))?;
        Self::new(
            address.parse()?,
            prefix
                .parse()
                .map_err(|_| AddressError("invalid CIDR prefix"))?,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressError(pub &'static str);

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for AddressError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_addresses_round_trip_and_cidr_matches() {
        let mac: MacAddress = "02:1a:2b:03:04:ff".parse().unwrap();
        assert_eq!(mac.to_string(), "02:1a:2b:03:04:ff");
        let network: Cidr = "192.168.4.77/24".parse().unwrap();
        assert_eq!(network.to_string(), "192.168.4.0/24");
        assert!(network.contains("192.168.4.250".parse().unwrap()));
        assert!(!network.contains("192.168.5.1".parse().unwrap()));
        let v6: Cidr = "2001:db8:abcd::12/48".parse().unwrap();
        assert!(v6.contains("2001:db8:abcd::ffff".parse().unwrap()));
        assert!(!v6.contains("2001:db8:abce::1".parse().unwrap()));
    }
}
