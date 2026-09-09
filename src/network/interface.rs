use super::{
    address::{IpAddress, Ipv4Address, Ipv6Address, MacAddress},
    InterfaceId,
};
use std::{
    collections::HashMap,
    ffi::CStr,
    io,
    os::raw::{c_char, c_int, c_uint, c_void},
};
#[cfg(target_os = "linux")]
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceKind {
    Wifi,
    Ethernet,
    Loopback,
    Tunnel,
    Virtual,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterfaceState {
    pub up: bool,
    pub running: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InterfaceStatistics {
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub received_packets: u64,
    pub transmitted_packets: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterfaceAddress {
    pub address: IpAddress,
    pub prefix: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkInterface {
    pub id: InterfaceId,
    pub kind: InterfaceKind,
    pub name: String,
    pub index: u32,
    pub mac: Option<MacAddress>,
    pub mtu: Option<u32>,
    pub state: InterfaceState,
    pub addresses: Vec<InterfaceAddress>,
    pub statistics: Option<InterfaceStatistics>,
}

/// Read the kernel's structured interface-address list. Linux enriches it from
/// sysfs. macOS uses the same getifaddrs ABI and leaves unavailable counters as
/// unknown instead of fabricating zeroes.
pub fn discover() -> io::Result<Vec<NetworkInterface>> {
    let rows = getifaddrs_rows()?;
    let mut interfaces = Vec::<NetworkInterface>::new();
    let mut indexes = HashMap::<u32, usize>::new();
    for row in rows {
        let index = unsafe { if_nametoindex(row.name.as_ptr().cast()) };
        if index == 0 {
            continue;
        }
        let position = *indexes.entry(index).or_insert_with(|| {
            let position = interfaces.len();
            let name = String::from_utf8_lossy(&row.name[..row.name.len() - 1]).into_owned();
            interfaces.push(NetworkInterface {
                id: InterfaceId(index),
                kind: classify(&name),
                name: name.clone(),
                index,
                mac: None,
                mtu: read_linux_u32(&name, "mtu"),
                state: InterfaceState {
                    up: row.flags & IFF_UP != 0,
                    running: row.flags & IFF_RUNNING != 0,
                },
                addresses: Vec::new(),
                statistics: linux_statistics(&name),
            });
            position
        });
        if let Some(mac) = row.mac {
            interfaces[position].mac = Some(mac);
        }
        if let Some(address) = row.address {
            if !interfaces[position].addresses.contains(&address) {
                interfaces[position].addresses.push(address);
            }
        }
    }
    for interface in &mut interfaces {
        interface.addresses.sort_by_key(|address| address.address);
    }
    interfaces.sort_by_key(|interface| interface.index);
    Ok(interfaces)
}

pub fn index_for_name(name: &str) -> Option<InterfaceId> {
    let mut bytes = name.as_bytes().to_vec();
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    bytes.push(0);
    let index = unsafe { if_nametoindex(bytes.as_ptr().cast()) };
    (index != 0).then_some(InterfaceId(index))
}

fn classify(name: &str) -> InterfaceKind {
    if name == "lo" || name.starts_with("lo0") {
        InterfaceKind::Loopback
    } else if name.starts_with("wl") || name.starts_with("awdl") || name.starts_with("llw") {
        InterfaceKind::Wifi
    } else if name.starts_with("utun")
        || name.starts_with("tun")
        || name.starts_with("tap")
        || name.starts_with("ipsec")
    {
        InterfaceKind::Tunnel
    } else if name.starts_with("bridge")
        || name.starts_with("docker")
        || name.starts_with("veth")
        || name.starts_with("vmnet")
    {
        InterfaceKind::Virtual
    } else if name.starts_with("eth") || name.starts_with("en") {
        InterfaceKind::Ethernet
    } else {
        InterfaceKind::Unknown
    }
}

#[derive(Debug)]
struct RawRow {
    name: Vec<u8>,
    flags: u32,
    mac: Option<MacAddress>,
    address: Option<InterfaceAddress>,
}

#[repr(C)]
struct IfAddrs {
    next: *mut IfAddrs,
    name: *mut c_char,
    flags: c_uint,
    address: *mut SockAddr,
    netmask: *mut SockAddr,
    destination: *mut SockAddr,
    data: *mut c_void,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct SockAddr {
    family: u16,
    data: [u8; 14],
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct SockAddr {
    length: u8,
    family: u8,
    data: [u8; 14],
}

unsafe extern "C" {
    fn getifaddrs(addresses: *mut *mut IfAddrs) -> c_int;
    fn freeifaddrs(addresses: *mut IfAddrs);
    fn if_nametoindex(name: *const c_char) -> c_uint;
}

#[cfg(target_os = "linux")]
const AF_INET: u16 = 2;
#[cfg(target_os = "linux")]
const AF_INET6: u16 = 10;
#[cfg(target_os = "linux")]
const AF_LINK_LAYER: u16 = 17;
#[cfg(target_os = "macos")]
const AF_INET: u16 = 2;
#[cfg(target_os = "macos")]
const AF_INET6: u16 = 30;
#[cfg(target_os = "macos")]
const AF_LINK_LAYER: u16 = 18;
const IFF_UP: u32 = 0x1;
const IFF_RUNNING: u32 = 0x40;

fn getifaddrs_rows() -> io::Result<Vec<RawRow>> {
    let mut head = std::ptr::null_mut();
    if unsafe { getifaddrs(&mut head) } != 0 {
        return Err(io::Error::last_os_error());
    }
    struct Guard(*mut IfAddrs);
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { freeifaddrs(self.0) };
        }
    }
    let _guard = Guard(head);
    let mut rows = Vec::new();
    let mut current = head;
    for _ in 0..4096 {
        if current.is_null() {
            return Ok(rows);
        }
        let item = unsafe { &*current };
        if !item.name.is_null() {
            let name = unsafe { CStr::from_ptr(item.name) }
                .to_bytes_with_nul()
                .to_vec();
            let family = sockaddr_family(item.address);
            rows.push(RawRow {
                name,
                flags: item.flags,
                mac: parse_mac(item.address, family),
                address: parse_ip(item.address, item.netmask, family),
            });
        }
        current = item.next;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "interface list exceeds 4096 records",
    ))
}

fn sockaddr_family(address: *const SockAddr) -> u16 {
    if address.is_null() {
        return 0;
    }
    #[cfg(target_os = "linux")]
    return unsafe { (*address).family };
    #[cfg(target_os = "macos")]
    return u16::from(unsafe { (*address).family });
}

#[cfg(target_os = "linux")]
fn parse_ip(
    address: *const SockAddr,
    netmask: *const SockAddr,
    family: u16,
) -> Option<InterfaceAddress> {
    if address.is_null() || netmask.is_null() {
        return None;
    }
    unsafe {
        match family {
            AF_INET => {
                let bytes = std::slice::from_raw_parts(address.cast::<u8>().add(4), 4);
                let mask = std::slice::from_raw_parts(netmask.cast::<u8>().add(4), 4);
                Some(InterfaceAddress {
                    address: IpAddress::V4(Ipv4Address::new(bytes.try_into().ok()?)),
                    prefix: prefix(mask)?,
                })
            }
            AF_INET6 => {
                let bytes = std::slice::from_raw_parts(address.cast::<u8>().add(8), 16);
                let mask = std::slice::from_raw_parts(netmask.cast::<u8>().add(8), 16);
                Some(InterfaceAddress {
                    address: IpAddress::V6(Ipv6Address::new(bytes.try_into().ok()?)),
                    prefix: prefix(mask)?,
                })
            }
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
fn parse_ip(
    address: *const SockAddr,
    netmask: *const SockAddr,
    family: u16,
) -> Option<InterfaceAddress> {
    if address.is_null() || netmask.is_null() {
        return None;
    }
    unsafe {
        match family {
            AF_INET => {
                let bytes = std::slice::from_raw_parts(address.cast::<u8>().add(4), 4);
                let mask = std::slice::from_raw_parts(netmask.cast::<u8>().add(4), 4);
                Some(InterfaceAddress {
                    address: IpAddress::V4(Ipv4Address::new(bytes.try_into().ok()?)),
                    prefix: prefix(mask)?,
                })
            }
            AF_INET6 => {
                let bytes = std::slice::from_raw_parts(address.cast::<u8>().add(8), 16);
                let mask = std::slice::from_raw_parts(netmask.cast::<u8>().add(8), 16);
                Some(InterfaceAddress {
                    address: IpAddress::V6(Ipv6Address::new(bytes.try_into().ok()?)),
                    prefix: prefix(mask)?,
                })
            }
            _ => None,
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_mac(address: *const SockAddr, family: u16) -> Option<MacAddress> {
    if address.is_null() || family != AF_LINK_LAYER {
        return None;
    }
    // sockaddr_ll: family(2), protocol(2), ifindex(4), hatype(2), pkttype(1),
    // halen(1), then address bytes.
    unsafe {
        let raw = address.cast::<u8>();
        if *raw.add(11) < 6 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(raw.add(12), 6);
        Some(MacAddress::new(bytes.try_into().ok()?))
    }
}

#[cfg(target_os = "macos")]
fn parse_mac(address: *const SockAddr, family: u16) -> Option<MacAddress> {
    if address.is_null() || family != AF_LINK_LAYER {
        return None;
    }
    // sockaddr_dl has nlen/alen at offsets 5/6 and variable link data at 8.
    unsafe {
        let raw = address.cast::<u8>();
        let total = usize::from(*raw);
        let name_len = usize::from(*raw.add(5));
        let address_len = usize::from(*raw.add(6));
        let start = 8 + name_len;
        if address_len < 6 || start + 6 > total {
            return None;
        }
        let bytes = std::slice::from_raw_parts(raw.add(start), 6);
        Some(MacAddress::new(bytes.try_into().ok()?))
    }
}

fn prefix(mask: &[u8]) -> Option<u8> {
    let mut count = 0_u8;
    let mut saw_zero = false;
    for byte in mask {
        for bit in (0..8).rev() {
            let set = byte & (1 << bit) != 0;
            if saw_zero && set {
                return None;
            }
            if set {
                count = count.checked_add(1)?;
            } else {
                saw_zero = true;
            }
        }
    }
    Some(count)
}

#[cfg(target_os = "linux")]
fn read_linux_u32(name: &str, field: &str) -> Option<u32> {
    fs::read_to_string(Path::new("/sys/class/net").join(name).join(field))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(not(target_os = "linux"))]
fn read_linux_u32(_name: &str, _field: &str) -> Option<u32> {
    None
}

#[cfg(target_os = "linux")]
fn linux_statistics(name: &str) -> Option<InterfaceStatistics> {
    let base = Path::new("/sys/class/net").join(name).join("statistics");
    let read = |field: &str| {
        fs::read_to_string(base.join(field))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Some(InterfaceStatistics {
        received_bytes: read("rx_bytes")?,
        transmitted_bytes: read("tx_bytes")?,
        received_packets: read("rx_packets")?,
        transmitted_packets: read("tx_packets")?,
    })
}

#[cfg(not(target_os = "linux"))]
fn linux_statistics(_name: &str) -> Option<InterfaceStatistics> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_discovery_includes_loopback_and_typed_address() {
        let interfaces = match discover() {
            Ok(interfaces) => interfaces,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("interface discovery failed: {error}"),
        };
        let loopback = interfaces
            .iter()
            .find(|interface| interface.kind == InterfaceKind::Loopback)
            .unwrap();
        assert!(loopback.index > 0);
        assert!(loopback
            .addresses
            .iter()
            .any(|address| match address.address {
                IpAddress::V4(address) => address.octets()[0] == 127,
                IpAddress::V6(address) =>
                    address.octets() == std::net::Ipv6Addr::LOCALHOST.octets(),
            }));
    }

    #[test]
    fn non_contiguous_netmask_is_rejected() {
        assert_eq!(prefix(&[255, 255, 0, 0]), Some(16));
        assert_eq!(prefix(&[255, 0, 255, 0]), None);
    }
}
