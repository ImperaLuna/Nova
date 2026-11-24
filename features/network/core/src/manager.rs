use nova_network_api::{LinkKind, LinkState, WifiBand, WifiSecurity};
use std::collections::HashMap;
use zbus::Connection;
use zbus::wire::{OwnedObjectPath, OwnedVariant};

pub const NM: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
pub const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
pub const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
pub const SETTINGS_CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub const WIRED: &str = "org.freedesktop.NetworkManager.Device.Wired";
pub const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const FAMILIES: [&str; 2] = ["ipv4", "ipv6"];
pub const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
pub const NO_OBJECT: &str = "/";

const DEVICE_TYPE_ETHERNET: u32 = 1;
const DEVICE_TYPE_WIFI: u32 = 2;
const DEVICE_STATE_DISCONNECTED: u32 = 30;
const DEVICE_STATE_ACTIVATED: u32 = 100;
pub const ACTIVE_STATE_ACTIVATED: u32 = 2;
pub const ACTIVE_STATE_DEACTIVATED: u32 = 4;
const AP_FLAG_PRIVACY: u32 = 0x1;
const AP_KEY_MGMT_PSK: u32 = 0x100;
const AP_KEY_MGMT_802_1X: u32 = 0x200;
const AP_KEY_MGMT_SAE: u32 = 0x400;
const AP_KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;
pub const TYPE_WIFI: &str = "802-11-wireless";
pub const TYPE_ETHERNET: &str = "802-3-ethernet";
const KBIT_PER_MBIT: u32 = 1000;

pub type Settings = HashMap<String, HashMap<String, OwnedVariant>>;
type Properties = HashMap<String, OwnedVariant>;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Reading {
    pub primary: Option<LinkKind>,
    pub wired: Vec<WiredDevice>,
    pub wifi: Option<WifiDevice>,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WiredDevice {
    pub path: OwnedObjectPath,
    pub interface: String,
    pub state: LinkState,
    pub connection: Option<String>,
    pub speed_mbps: Option<u32>,
    pub active: Option<OwnedObjectPath>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WifiDevice {
    pub path: OwnedObjectPath,
    pub interface: String,
    pub enabled: bool,
    pub blocked: bool,
    pub state: LinkState,
    pub active: Option<OwnedObjectPath>,
    pub network: Option<String>,
    pub speed_mbps: Option<u32>,
    pub last_scan: i64,
    pub access_points: Vec<AccessPoint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessPoint {
    pub ssid: Vec<u8>,
    pub strength: u8,
    pub security: WifiSecurity,
    pub key_mgmt: Option<&'static str>,
    pub band: Option<WifiBand>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub path: OwnedObjectPath,
    pub kind: String,
    pub ssid: Option<Vec<u8>>,
}

pub async fn read(connection: &Connection) -> zbus::Result<Reading> {
    let manager = get_all(connection, NM_PATH, NM).await?;
    let wifi_enabled = flag(&manager, "WirelessEnabled");
    let wifi_blocked =
        !manager.get("WirelessHardwareEnabled").and_then(|value| bool::try_from(value).ok()).unwrap_or(true);
    let primary = match path(&manager, "PrimaryConnection") {
        Some(active) => {
            get_all(connection, active.as_str(), ACTIVE).await.ok().map(|active| kind(&text(&active, "Type")))
        }
        None => None,
    };
    let devices: Vec<OwnedObjectPath> =
        connection.call_method(Some(NM), NM_PATH, Some(NM), "GetDevices", &()).await?.body().deserialize()?;
    let mut reading = Reading { primary, ..Reading::default() };
    for device_path in devices {
        let Ok(device) = get_all(connection, device_path.as_str(), DEVICE).await else { continue };
        if !flag(&device, "Managed") || !flag(&device, "Real") {
            continue;
        }
        let interface = text(&device, "Interface");
        let device_state = number(&device, "State");
        let state = link_state(device_state);
        let active = path(&device, "ActiveConnection");
        let active_id = match &active {
            Some(active) => get_all(connection, active.as_str(), ACTIVE).await.ok().map(|active| text(&active, "Id")),
            None => None,
        };
        match number(&device, "DeviceType") {
            DEVICE_TYPE_ETHERNET => {
                let wired = get_all(connection, device_path.as_str(), WIRED).await.unwrap_or_default();
                let speed = number(&wired, "Speed");
                reading.wired.push(WiredDevice {
                    path: device_path,
                    interface,
                    state,
                    connection: active_id.filter(|id| !id.is_empty()),
                    speed_mbps: (state == LinkState::Connected && speed > 0).then_some(speed),
                    active,
                });
            }
            DEVICE_TYPE_WIFI if reading.wifi.is_none() => {
                let wireless = get_all(connection, device_path.as_str(), WIRELESS).await.unwrap_or_default();
                let active_point = path(&wireless, "ActiveAccessPoint");
                let points = wireless
                    .get("AccessPoints")
                    .and_then(|value| Vec::<OwnedObjectPath>::try_from(value.try_clone().ok()?).ok())
                    .unwrap_or_default();
                let mut access_points = Vec::new();
                for point in points {
                    let Ok(properties) = get_all(connection, point.as_str(), ACCESS_POINT).await else { continue };
                    access_points.push(access_point(&properties, active_point.as_ref() == Some(&point)));
                }
                let bitrate = number(&wireless, "Bitrate");
                let network = access_points.iter().find(|point| point.active).map(|point| ssid_text(&point.ssid));
                reading.wifi = Some(WifiDevice {
                    path: device_path,
                    interface,
                    enabled: wifi_enabled,
                    blocked: wifi_blocked,
                    state,
                    active,
                    network: network.or(active_id).filter(|name| !name.is_empty()),
                    speed_mbps: (state == LinkState::Connected && bitrate > 0).then_some(bitrate / KBIT_PER_MBIT),
                    last_scan: wireless.get("LastScan").and_then(|value| i64::try_from(value).ok()).unwrap_or(-1),
                    access_points,
                });
            }
            _ => {}
        }
    }
    reading.profiles = profiles(connection).await;
    Ok(reading)
}

async fn profiles(connection: &Connection) -> Vec<Profile> {
    let Ok(paths) = async {
        connection
            .call_method(Some(NM), SETTINGS_PATH, Some(SETTINGS), "ListConnections", &())
            .await?
            .body()
            .deserialize::<Vec<OwnedObjectPath>>()
    }
    .await
    else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    for path in paths {
        let Ok(settings) = settings(connection, &path).await else { continue };
        profiles.push(profile(path, &settings));
    }
    profiles
}

pub async fn settings(connection: &Connection, path: &OwnedObjectPath) -> zbus::Result<Settings> {
    connection
        .call_method(Some(NM), path.as_str(), Some(SETTINGS_CONNECTION), "GetSettings", &())
        .await?
        .body()
        .deserialize()
}

pub fn profile(path: OwnedObjectPath, settings: &Settings) -> Profile {
    let kind = setting_text(settings, "connection", "type").unwrap_or_default();
    let ssid = settings
        .get(TYPE_WIFI)
        .and_then(|wifi| wifi.get("ssid"))
        .and_then(|value| Vec::<u8>::try_from(value.try_clone().ok()?).ok());
    Profile { path, kind, ssid }
}

pub fn setting_text(settings: &Settings, setting: &str, key: &str) -> Option<String> {
    settings.get(setting)?.get(key).and_then(|value| String::try_from(value.try_clone().ok()?).ok())
}

pub fn route_metric(settings: &Settings, family: &str) -> Option<i64> {
    settings.get(family)?.get("route-metric").and_then(|value| i64::try_from(value).ok()).filter(|metric| *metric >= 0)
}

pub async fn get_all(connection: &Connection, path: &str, interface: &str) -> zbus::Result<Properties> {
    connection.call_method(Some(NM), path, Some(PROPERTIES), "GetAll", &(interface,)).await?.body().deserialize()
}

fn access_point(properties: &Properties, active: bool) -> AccessPoint {
    let ssid =
        properties.get("Ssid").and_then(|value| Vec::<u8>::try_from(value.try_clone().ok()?).ok()).unwrap_or_default();
    let (security, key_mgmt) =
        security(number(properties, "Flags"), number(properties, "WpaFlags"), number(properties, "RsnFlags"));
    AccessPoint {
        ssid,
        strength: properties.get("Strength").and_then(|value| u8::try_from(value).ok()).unwrap_or(0).min(100),
        security,
        key_mgmt,
        band: band(number(properties, "Frequency")),
        active,
    }
}

pub fn security(flags: u32, wpa: u32, rsn: u32) -> (WifiSecurity, Option<&'static str>) {
    let both = wpa | rsn;
    if both & (AP_KEY_MGMT_802_1X | AP_KEY_MGMT_EAP_SUITE_B_192) != 0 {
        return (WifiSecurity::Enterprise, None);
    }
    if rsn & AP_KEY_MGMT_SAE != 0 {
        let key_mgmt = if both & AP_KEY_MGMT_PSK != 0 { "wpa-psk" } else { "sae" };
        return (WifiSecurity::Wpa3, Some(key_mgmt));
    }
    if rsn & AP_KEY_MGMT_PSK != 0 {
        return (WifiSecurity::Wpa2, Some("wpa-psk"));
    }
    if wpa & AP_KEY_MGMT_PSK != 0 {
        return (WifiSecurity::Wpa, Some("wpa-psk"));
    }
    if both == 0 && flags & AP_FLAG_PRIVACY != 0 {
        return (WifiSecurity::Wep, None);
    }
    (WifiSecurity::Open, None)
}

fn band(frequency_mhz: u32) -> Option<WifiBand> {
    match frequency_mhz {
        2400..=2500 => Some(WifiBand::Ghz2_4),
        4900..=5899 => Some(WifiBand::Ghz5),
        5925..=7125 => Some(WifiBand::Ghz6),
        _ => None,
    }
}

fn link_state(device_state: u32) -> LinkState {
    match device_state {
        DEVICE_STATE_ACTIVATED => LinkState::Connected,
        state if state < DEVICE_STATE_DISCONNECTED => LinkState::Unavailable,
        DEVICE_STATE_DISCONNECTED => LinkState::Disconnected,
        state if state < DEVICE_STATE_ACTIVATED => LinkState::Connecting,
        _ => LinkState::Disconnected,
    }
}

pub fn kind(connection_type: &str) -> LinkKind {
    match connection_type {
        TYPE_ETHERNET => LinkKind::Wired,
        TYPE_WIFI => LinkKind::Wifi,
        _ => LinkKind::Other,
    }
}

pub fn ssid_text(ssid: &[u8]) -> String {
    String::from_utf8_lossy(ssid).chars().filter(|c| !c.is_control()).collect()
}

pub fn names_ssid(name: &str, ssid: &[u8]) -> bool {
    ssid == name.as_bytes() || ssid_text(ssid) == name
}

fn text(properties: &Properties, name: &str) -> String {
    properties.get(name).and_then(|value| String::try_from(value.try_clone().ok()?).ok()).unwrap_or_default()
}

fn number(properties: &Properties, name: &str) -> u32 {
    properties.get(name).and_then(|value| u32::try_from(value).ok()).unwrap_or(0)
}

fn flag(properties: &Properties, name: &str) -> bool {
    properties.get(name).and_then(|value| bool::try_from(value).ok()).unwrap_or(false)
}

fn path(properties: &Properties, name: &str) -> Option<OwnedObjectPath> {
    properties
        .get(name)
        .and_then(|value| OwnedObjectPath::try_from(value.try_clone().ok()?).ok())
        .filter(|path| path.as_str() != NO_OBJECT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_isnt_utf8_still_names_its_network() {
        let ssid = b"Caf\xe9\x07";
        assert_eq!(ssid_text(ssid), "Caf\u{fffd}");
        assert!(names_ssid(&ssid_text(ssid), ssid), "commands come back with the shown name");
        assert!(names_ssid("Home", b"Home"));
        assert!(!names_ssid("Home", b"Home-2G"));
    }

    #[test]
    fn access_points_are_secured_by_their_best_method() {
        assert_eq!(security(3, 0, 392), (WifiSecurity::Wpa2, Some("wpa-psk")));
        assert_eq!(security(1, 0, 0x400 | 0x88), (WifiSecurity::Wpa3, Some("sae")));
        assert_eq!(security(1, 0, 0x500 | 0x88), (WifiSecurity::Wpa3, Some("wpa-psk")));
        assert_eq!(security(1, 0, 0x200 | 0x88), (WifiSecurity::Enterprise, None));
        assert_eq!(security(1, 0, 0), (WifiSecurity::Wep, None));
        assert_eq!(security(0, 0, 0), (WifiSecurity::Open, None));
        assert_eq!(security(1, 0x108, 0), (WifiSecurity::Wpa, Some("wpa-psk")));
    }

    #[test]
    fn device_states_become_link_states() {
        assert_eq!(link_state(20), LinkState::Unavailable);
        assert_eq!(link_state(30), LinkState::Disconnected);
        assert_eq!(link_state(60), LinkState::Connecting);
        assert_eq!(link_state(100), LinkState::Connected);
        assert_eq!(link_state(120), LinkState::Disconnected);
    }
}
