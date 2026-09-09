//! Wi-Fi link state, obtained through bounded structured operating-system
//! inspection. Missing privacy-protected fields remain unknown.
use super::{address::MacAddress, InterfaceId};
use std::{fmt, io};
#[cfg(target_os = "macos")]
use std::{path::Path, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Ghz2,
    Ghz5,
    Ghz6,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityMode {
    Open,
    Wep,
    Wpa,
    Wpa2,
    Wpa3,
    Enterprise,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ssid {
    bytes: [u8; 32],
    length: u8,
}

impl Ssid {
    pub fn from_utf8(value: &str) -> io::Result<Self> {
        if value.len() > 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SSID exceeds 32-byte link-layer limit",
            ));
        }
        let mut bytes = [0; 32];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Ok(Self {
            bytes,
            length: value.len() as u8,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length as usize]
    }
}

impl fmt::Display for Ssid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        String::from_utf8_lossy(self.as_bytes()).fmt(formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiLink {
    pub interface: Option<InterfaceId>,
    pub interface_name: String,
    pub state: ConnectionState,
    pub ssid: Option<Ssid>,
    pub bssid: Option<MacAddress>,
    pub channel: Option<u16>,
    pub band: Option<Band>,
    pub signal_dbm: Option<i16>,
    pub noise_dbm: Option<i16>,
    pub link_rate_mbps: Option<u32>,
    pub security: SecurityMode,
}

#[cfg(target_os = "macos")]
pub fn discover() -> io::Result<Vec<WifiLink>> {
    let result = crate::process::run(
        Path::new("/"),
        "/usr/sbin/system_profiler",
        &["-json", "-detailLevel", "mini", "SPAirPortDataType"],
        None,
        Duration::from_secs(10),
    );
    if !result.success {
        return Err(io::Error::other(format!(
            "Wi-Fi inspection failed: {}",
            result.stderr.trim()
        )));
    }
    parse_system_profiler(&result.stdout, super::interface::index_for_name)
}

#[cfg(not(target_os = "macos"))]
pub fn discover() -> io::Result<Vec<WifiLink>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "structured Wi-Fi discovery currently supports macOS",
    ))
}

#[cfg(any(test, target_os = "macos"))]
fn parse_system_profiler(
    text: &str,
    interface_id: impl Fn(&str) -> Option<InterfaceId>,
) -> io::Result<Vec<WifiLink>> {
    let root = crate::json::parse(text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let groups = root
        .get("SPAirPortDataType")
        .and_then(crate::json::Value::as_array)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Wi-Fi data array"))?;
    let mut links = Vec::new();
    for interface in groups
        .iter()
        .filter_map(|group| group.get("spairport_airport_interfaces"))
        .filter_map(crate::json::Value::as_array)
        .flatten()
        .take(64)
    {
        let name = string(interface, &["_name", "name"]).unwrap_or("");
        if name.is_empty() || name.len() > 128 {
            continue;
        }
        let current = interface
            .get("spairport_current_network_information")
            .and_then(crate::json::Value::as_object)
            .and_then(|networks| networks.iter().next());
        let (ssid, details) = current.map_or((None, None), |(ssid, details)| {
            (Ssid::from_utf8(ssid).ok(), Some(details))
        });
        let channel = details
            .and_then(|value| string(value, &["spairport_network_channel", "channel"]))
            .and_then(first_u16);
        let band = details
            .and_then(|value| string(value, &["spairport_network_channel", "channel"]))
            .map(parse_band);
        let (signal_dbm, noise_dbm) = details
            .and_then(|value| string(value, &["spairport_signal_noise", "signal_noise"]))
            .map(parse_signal_noise)
            .unwrap_or((None, None));
        let security_text = details
            .and_then(|value| string(value, &["spairport_security_mode", "security"]))
            .unwrap_or("");
        links.push(WifiLink {
            interface: interface_id(name),
            interface_name: name.to_owned(),
            state: if current.is_some() {
                ConnectionState::Connected
            } else {
                ConnectionState::Disconnected
            },
            ssid,
            bssid: details
                .and_then(|value| string(value, &["spairport_network_bssid", "bssid"]))
                .and_then(|value| value.parse().ok()),
            channel,
            band,
            signal_dbm,
            noise_dbm,
            link_rate_mbps: details
                .and_then(|value| string(value, &["spairport_network_rate", "last_tx_rate"]))
                .and_then(first_u32),
            security: parse_security(security_text),
        });
    }
    links.sort_by(|left, right| left.interface_name.cmp(&right.interface_name));
    Ok(links)
}

#[cfg(any(test, target_os = "macos"))]
fn string<'a>(value: &'a crate::json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(crate::json::Value::as_str))
}

#[cfg(any(test, target_os = "macos"))]
fn first_u16(value: &str) -> Option<u16> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .find(|field| !field.is_empty())?
        .parse()
        .ok()
}

#[cfg(any(test, target_os = "macos"))]
fn first_u32(value: &str) -> Option<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .find(|field| !field.is_empty())?
        .parse()
        .ok()
}

#[cfg(any(test, target_os = "macos"))]
fn parse_band(value: &str) -> Band {
    if value.contains("6GHz") || value.contains("6 GHz") {
        Band::Ghz6
    } else if value.contains("5GHz") || value.contains("5 GHz") {
        Band::Ghz5
    } else if value.contains("2GHz") || value.contains("2 GHz") {
        Band::Ghz2
    } else {
        Band::Unknown
    }
}

#[cfg(any(test, target_os = "macos"))]
fn parse_signal_noise(value: &str) -> (Option<i16>, Option<i16>) {
    let values = value
        .split(|character: char| !(character.is_ascii_digit() || character == '-'))
        .filter_map(|field| field.parse::<i16>().ok())
        .take(2)
        .collect::<Vec<_>>();
    (values.first().copied(), values.get(1).copied())
}

#[cfg(any(test, target_os = "macos"))]
fn parse_security(value: &str) -> SecurityMode {
    let value = value.to_ascii_lowercase();
    if value.is_empty() {
        SecurityMode::Unknown
    } else if value.contains("enterprise") || value.contains("802.1x") {
        SecurityMode::Enterprise
    } else if value.contains("wpa3") {
        SecurityMode::Wpa3
    } else if value.contains("wpa2") {
        SecurityMode::Wpa2
    } else if value.contains("wpa") {
        SecurityMode::Wpa
    } else if value.contains("wep") {
        SecurityMode::Wep
    } else if value.contains("none") || value.contains("open") {
        SecurityMode::Open
    } else {
        SecurityMode::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_link_state_and_tolerates_redaction() {
        let fixture = r#"{
          "SPAirPortDataType": [{
            "spairport_airport_interfaces": [{
              "_name": "en0",
              "spairport_current_network_information": {
                "Home": {
                  "spairport_network_bssid": "aa:bb:cc:dd:ee:ff",
                  "spairport_network_channel": "44 (5GHz, 80MHz)",
                  "spairport_signal_noise": "-48 dBm / -92 dBm",
                  "spairport_network_rate": "866",
                  "spairport_security_mode": "WPA3 Personal"
                }
              }
            }, {"_name": "awdl0"}]
          }]
        }"#;
        let links =
            parse_system_profiler(fixture, |name| (name == "en0").then_some(InterfaceId(7)))
                .unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].state, ConnectionState::Disconnected);
        assert_eq!(links[1].interface, Some(InterfaceId(7)));
        assert_eq!(links[1].ssid.unwrap().as_bytes(), b"Home");
        assert_eq!(links[1].channel, Some(44));
        assert_eq!(links[1].band, Some(Band::Ghz5));
        assert_eq!(links[1].signal_dbm, Some(-48));
        assert_eq!(links[1].noise_dbm, Some(-92));
        assert_eq!(links[1].link_rate_mbps, Some(866));
        assert_eq!(links[1].security, SecurityMode::Wpa3);
    }

    #[test]
    fn ssid_and_json_input_are_bounded_and_validated() {
        assert!(Ssid::from_utf8(&"x".repeat(33)).is_err());
        assert!(parse_system_profiler("{}", |_| None).is_err());
    }
}
