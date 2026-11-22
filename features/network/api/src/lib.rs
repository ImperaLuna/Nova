//! The network feature's messages: the machine's wired and Wi-Fi links
//! from NetworkManager, the Wi-Fi networks in reach, connecting to them,
//! which link to prefer, and the traffic, for the quick settings network
//! tile and page.
//!
//! A client sends a [`Command`] (`set_wifi`, `connect_wifi`, ...); novad
//! publishes an [`Event`] on [`TOPIC`]: `network` right after subscribing
//! and whenever a link changes, `network_traffic` every second while some
//! connection watches it. The work is `nova-network`
//! (`features/network/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The topic a client subscribes to for [`Event::Network`] and
/// [`Event::NetworkTraffic`].
pub const TOPIC: Topic = Topic::named("network");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Turns Wi-Fi on or off (feature `network`).
    SetWifi { enabled: bool },
    /// Asks the Wi-Fi card to look for networks now (feature `network`),
    /// when the network page opens. `scanning` says when it's done.
    ScanWifi,
    /// Connects the Wi-Fi card to `ssid` (feature `network`): with its
    /// saved profile, else a new one, with `password` for one that needs
    /// it. Answers once NetworkManager has started; how it ends comes as
    /// the [`Wifi::attempt`]. A password for a saved network replaces the
    /// saved one. A new network it never got onto isn't kept.
    ConnectWifi {
        ssid: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<WifiPassword>,
    },
    /// Disconnects the Wi-Fi card from the network it's on (feature
    /// `network`). That network doesn't connect by itself again until it's
    /// picked or the machine restarts; others still do.
    DisconnectWifi,
    /// Forgets the saved profiles of the Wi-Fi network `ssid` (feature
    /// `network`), which disconnects from it first if it's on it.
    ForgetWifi { ssid: String },
    /// Which link carries traffic while both a cable and Wi-Fi are
    /// connected (feature `network`); the other stays connected as a
    /// backup. Remembered across restarts. NetworkManager's saved profiles
    /// aren't changed: novad lowers the link's route metric while it's
    /// connected.
    PreferNetwork { kind: LinkKind },
    /// Starts or stops this connection watching network traffic (feature
    /// `network`): while any does, novad sends `network_traffic` every
    /// second. A connection that closes stops watching.
    WatchNetworkTraffic { watching: bool },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Wired and Wi-Fi links from NetworkManager (feature `network`). Topic
    /// [`TOPIC`], sent right after subscribing and whenever a link,
    /// a network in reach or a connection attempt changes.
    Network(Network),
    /// The traffic over the machine's network links, every
    /// [`NETWORK_TRAFFIC_INTERVAL_SECS`] while some connection watches it
    /// (`watch_network_traffic`, feature `network`). Topic [`TOPIC`].
    NetworkTraffic(NetworkTraffic),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// The machine's network links as NetworkManager has them (feature
/// `network`). While NetworkManager doesn't run, `running` is false and the
/// rest is empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub running: bool,
    /// The kind of link traffic goes over now (NetworkManager's primary
    /// connection), `None` while offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<LinkKind>,
    /// Which link should carry traffic while both a cable and Wi-Fi are
    /// connected: the user's pick (`prefer_network`), else, while both are
    /// connected, the one that does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer: Option<LinkKind>,
    /// Every Ethernet port NetworkManager manages.
    pub wired: Vec<WiredLink>,
    /// The first Wi-Fi card NetworkManager manages, `None` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Wifi>,
}

/// What a network link is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Wired,
    Wifi,
    /// Anything else NetworkManager connects through (a phone over USB, a
    /// mobile broadband modem, a VPN it routes everything over).
    Other,
}

/// Where a link is in connecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// It can't connect now: no cable in the port, or Wi-Fi turned off.
    Unavailable,
    Disconnected,
    Connecting,
    Connected,
}

/// One Ethernet port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WiredLink {
    /// The kernel's name for it, like `enp5s0`.
    pub interface: String,
    pub state: LinkState,
    /// The connection profile in use, like `Wired connection 1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The link speed the cable negotiated, while connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<u32>,
}

/// The Wi-Fi card and the networks in its reach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wifi {
    pub interface: String,
    /// Wi-Fi's switch (`set_wifi`).
    pub enabled: bool,
    /// Turned off by a hardware switch or an airplane mode key: `set_wifi`
    /// can't turn it on.
    pub blocked: bool,
    pub state: LinkState,
    /// The network it's on, or connecting to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// The speed it's talking to the access point at, while connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<u32>,
    /// A scan was asked for (`scan_wifi`) and hasn't finished.
    pub scanning: bool,
    /// The networks in reach, each name once, the one it's on first, then
    /// the strongest. Networks that hide their name aren't listed.
    pub networks: Vec<WifiNetwork>,
    /// How the last `connect_wifi` went, until the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<WifiAttempt>,
}

/// A Wi-Fi network in reach, from its strongest access point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiNetwork {
    /// Its name (SSID), as text: bytes that aren't UTF-8 are replaced.
    pub ssid: String,
    /// Signal strength, 0 to 100.
    pub strength: u8,
    pub security: WifiSecurity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<WifiBand>,
    /// NetworkManager has a profile for it, so it connects without asking
    /// for a password.
    pub saved: bool,
    /// The card is on it now.
    pub connected: bool,
}

/// How a Wi-Fi network is secured, its best method when it offers several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiSecurity {
    /// No password (including Enhanced Open, which encrypts without one).
    Open,
    /// The old WEP: Nova doesn't connect to these.
    Wep,
    Wpa,
    Wpa2,
    Wpa3,
    /// 802.1X (work and school networks, like eduroam): set up in the
    /// system's network settings, Nova doesn't connect to these.
    Enterprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiBand {
    #[serde(rename = "2.4ghz")]
    Ghz2_4,
    #[serde(rename = "5ghz")]
    Ghz5,
    #[serde(rename = "6ghz")]
    Ghz6,
}

/// How a `connect_wifi` is going.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiAttempt {
    pub ssid: String,
    pub outcome: AttemptOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Connecting,
    Connected,
    /// The network turned the password down, or none was given for a
    /// network that needs one: ask for it.
    WrongPassword,
    /// It failed some other way (out of reach, no address).
    Failed,
}

/// How often traffic is sampled while watched, and the step of its history.
pub const NETWORK_TRAFFIC_INTERVAL_SECS: u64 = 1;
/// How far back a traffic history goes.
pub const NETWORK_TRAFFIC_HISTORY_SECS: u64 = 60;

/// Bytes over the machine's wired and Wi-Fi links together (not over
/// loopback, bridges or tunnels, whose traffic also crosses one of those).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkTraffic {
    /// Bytes per second down and up, one per
    /// [`NETWORK_TRAFFIC_INTERVAL_SECS`], oldest first, up to
    /// [`NETWORK_TRAFFIC_HISTORY_SECS`] back; the last is now.
    pub down: Vec<u64>,
    pub up: Vec<u64>,
    /// Bytes since local midnight, counted while novad ran.
    pub today_down: u64,
    pub today_up: u64,
}

/// A Wi-Fi password, on its way from the UI to NetworkManager. It never
/// shows in a log: its `Debug` hides it, and it has no `Display`. It's
/// serialized only to send it (the `nova` CLI, tests).
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct WifiPassword(String);

impl WifiPassword {
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for WifiPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WifiPassword(<redacted>)")
    }
}

impl Serialize for WifiPassword {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// The longest network name a command takes, in bytes (802.11's limit).
pub const WIFI_SSID_MAX_BYTES: usize = 32;
/// The longest password a command takes, in bytes: WPA's passphrases stop
/// at 63 (64 as hex), WPA3's have no limit of their own.
pub const WIFI_PASSWORD_MAX_BYTES: usize = 128;
/// The shortest WPA passphrase, in bytes (WPA3's SAE takes shorter ones, but
/// a shorter one is never a WPA2 key, and networks offering both are common).
pub const WPA_PASSWORD_MIN_BYTES: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn network() -> Network {
        Network {
            running: true,
            primary: Some(LinkKind::Wifi),
            prefer: Some(LinkKind::Wifi),
            wired: vec![WiredLink {
                interface: "enp5s0".into(),
                state: LinkState::Connected,
                connection: Some("Wired connection 1".into()),
                speed_mbps: Some(1000),
            }],
            wifi: Some(Wifi {
                interface: "wlp6s0".into(),
                enabled: true,
                blocked: false,
                state: LinkState::Connected,
                network: Some("Home".into()),
                speed_mbps: Some(585),
                scanning: false,
                networks: vec![
                    WifiNetwork {
                        ssid: "Home".into(),
                        strength: 84,
                        security: WifiSecurity::Wpa2,
                        band: Some(WifiBand::Ghz2_4),
                        saved: true,
                        connected: true,
                    },
                    WifiNetwork {
                        ssid: "Cafe".into(),
                        strength: 31,
                        security: WifiSecurity::Open,
                        band: Some(WifiBand::Ghz5),
                        saved: false,
                        connected: false,
                    },
                ],
                attempt: Some(WifiAttempt { ssid: "Home".into(), outcome: AttemptOutcome::Connected }),
            }),
        }
    }

    #[test]
    fn network_rejects_unknown_fields() {
        let network = serde_json::to_value(network()).unwrap();
        assert!(serde_json::from_value::<Network>(network.clone()).is_ok());
        let mut extra = network.clone();
        extra["wifi"]["networks"][0]["bssid"] = json!("02:00:00:00:00:08");
        assert!(serde_json::from_value::<Network>(extra).is_err());
    }

    #[test]
    fn wifi_passwords_stay_out_of_debug_output() {
        let command = Command::ConnectWifi { ssid: "Home".into(), password: Some(WifiPassword::new("hunter22")) };
        assert!(!format!("{command:?}").contains("hunter22"));
        let line = nova_carrier::encode(nova_carrier::Request { id: None, command }).unwrap();
        assert!(line.contains(r#""password":"hunter22""#), "{line}");
    }

    #[test]
    fn its_events_are_on_its_topic() {
        assert_eq!(Event::Network(network()).topic(), Some(TOPIC));
        let traffic = NetworkTraffic { down: vec![], up: vec![], today_down: 0, today_up: 0 };
        assert_eq!(Event::NetworkTraffic(traffic).topic(), Some(TOPIC));
    }
}
