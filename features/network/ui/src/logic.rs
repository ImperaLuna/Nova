//! What `ui/quick_settings/network.js` worked out, with no GPUI drawing in
//! it: from a network event (`None` before the first), which link is in
//! use, what the tile shows, each row's line and role, which networks are
//! listed, a password's length, and how traffic reads.

use nova_look::Glyph;
use nova_network_api::{
    AttemptOutcome, LinkKind, LinkState, Network, NetworkTraffic, WIFI_PASSWORD_MAX_BYTES, WPA_PASSWORD_MIN_BYTES,
    WifiBand, WifiNetwork, WifiSecurity, WiredLink,
};

/// The network a connect is going to now: the last attempt while it runs,
/// else the network the card is joining on its own.
pub fn connecting(network: Option<&Network>) -> Option<&str> {
    let wifi = network?.wifi.as_ref()?;
    if let Some(attempt) = wifi.attempt.as_ref().filter(|attempt| attempt.outcome == AttemptOutcome::Connecting) {
        return Some(&attempt.ssid);
    }
    match wifi.state {
        LinkState::Connecting => Some(wifi.network.as_deref().unwrap_or("")),
        _ => None,
    }
}

/// The Ethernet port the Wired row shows: the connected one, else one with
/// a cable in, else none (no cable: the row and Prefer are hidden).
pub fn wired_link(network: Option<&Network>) -> Option<&WiredLink> {
    let wired = network.map_or(&[][..], |network| network.wired.as_slice());
    wired
        .iter()
        .find(|link| link.state == LinkState::Connected)
        .or_else(|| wired.iter().find(|link| link.state != LinkState::Unavailable))
}

fn wired_up(network: Option<&Network>) -> bool {
    wired_link(network).is_some_and(|link| link.state == LinkState::Connected)
}

/// Wi-Fi's switch is on and nothing blocks it: the list shows.
pub fn wifi_on(network: Option<&Network>) -> bool {
    network.and_then(|network| network.wifi.as_ref()).is_some_and(|wifi| wifi.enabled && !wifi.blocked)
}

fn wifi_up(network: Option<&Network>) -> bool {
    wifi_on(network)
        && network.and_then(|network| network.wifi.as_ref()).is_some_and(|wifi| wifi.state == LinkState::Connected)
}

/// Prefer shows while a cable is connected on a machine with Wi-Fi, so the
/// pick is there before Wi-Fi connects too.
pub fn prefer_shown(network: Option<&Network>) -> bool {
    wired_up(network) && network.is_some_and(|network| network.wifi.is_some())
}

/// What a connected link's row says at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Traffic goes over it.
    Use,
    /// Connected too, but the other one carries traffic.
    Backup,
    /// Wi-Fi alone on a machine with no cable in.
    Connected,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Use => "In use",
            Role::Backup => "Backup",
            Role::Connected => "Connected",
        }
    }
}

/// With both connected, the one traffic goes over is in use and the other
/// the backup; alone, the Wired row is in use, and Wi-Fi is connected (or
/// in use beside a Wired row).
pub fn role(network: Option<&Network>, kind: LinkKind) -> Role {
    if wired_up(network) && wifi_up(network) {
        return match network.and_then(|network| network.primary) == Some(kind) {
            true => Role::Use,
            false => Role::Backup,
        };
    }
    if kind == LinkKind::Wired || wired_link(network).is_some() {
        return Role::Use;
    }
    Role::Connected
}

/// An icon of the network's: its line and, for Wi-Fi's, the solid dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkIcon {
    pub glyph: Glyph,
    pub fill: Option<Glyph>,
}

impl LinkIcon {
    fn of(glyph: Glyph) -> Self {
        let wifi = glyph == Glyph::WIFI || glyph == Glyph::WIFI_OFF;
        Self { glyph, fill: wifi.then_some(Glyph::WIFI_DOT) }
    }
}

/// The tile's icon: Wi-Fi while it connects, else the link in use, else
/// Wi-Fi or the port struck out.
pub fn tile_icon(network: Option<&Network>) -> LinkIcon {
    if connecting(network).is_some() {
        return LinkIcon::of(Glyph::WIFI);
    }
    let glyph = match network.and_then(|network| network.primary) {
        Some(LinkKind::Wired) => Glyph::WIRED,
        Some(LinkKind::Other) if !wifi_up(network) => Glyph::WIRED,
        Some(_) => Glyph::WIFI,
        None if network.is_some_and(|network| network.wifi.is_some()) => Glyph::WIFI_OFF,
        None => Glyph::WIRED_OFF,
    };
    LinkIcon::of(glyph)
}

/// The tile is lit while a link is up or coming up.
pub fn tile_on(network: Option<&Network>) -> bool {
    network.is_some_and(|network| network.primary.is_some()) || connecting(network).is_some()
}

/// The traffic card's icon: the link in use, or what's there struck out.
pub fn card_icon(network: Option<&Network>) -> LinkIcon {
    let has_wifi = network.is_some_and(|network| network.wifi.is_some());
    let glyph = match network.and_then(|network| network.primary) {
        Some(LinkKind::Wifi) => Glyph::WIFI,
        Some(_) => Glyph::WIRED,
        None if has_wifi => Glyph::WIFI_OFF,
        None => Glyph::WIRED_OFF,
    };
    LinkIcon::of(glyph)
}

/// The traffic card's title: the link in use, or "Not connected".
pub fn link_name(network: Option<&Network>) -> String {
    match network.and_then(|network| network.primary) {
        Some(LinkKind::Wired) => "Wired".into(),
        Some(LinkKind::Wifi) => network
            .and_then(|network| network.wifi.as_ref())
            .and_then(|wifi| wifi.network.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Wi-Fi".into()),
        Some(LinkKind::Other) => "Connected".into(),
        None => "Not connected".into(),
    }
}

/// The in-use link's speed, or 0 when it has none to give.
pub fn link_mbps(network: Option<&Network>) -> u32 {
    match network.and_then(|network| network.primary) {
        Some(LinkKind::Wired) => wired_link(network).and_then(|link| link.speed_mbps).unwrap_or(0),
        Some(LinkKind::Wifi) => {
            network.and_then(|network| network.wifi.as_ref()).and_then(|wifi| wifi.speed_mbps).unwrap_or(0)
        }
        _ => 0,
    }
}

const MBPS_PER_GBPS: u32 = 1000;

/// "1 Gb/s", "2.5 Gb/s", "866 Mb/s".
pub fn link_speed(mbps: u32) -> String {
    if mbps < MBPS_PER_GBPS {
        return format!("{mbps} Mb/s");
    }
    let gbps = format!("{:.1}", f64::from(mbps) / f64::from(MBPS_PER_GBPS));
    format!("{} Gb/s", gbps.strip_suffix(".0").unwrap_or(&gbps))
}

/// The Wired row's line: its port, and its speed when it has one.
pub fn wired_line(link: &WiredLink) -> String {
    match link.speed_mbps.filter(|mbps| *mbps > 0) {
        Some(mbps) => format!("{} · {}", link.interface, link_speed(mbps)),
        None => link.interface.clone(),
    }
}

/// The security a network row names: only the kinds that change what a
/// click does (no password, or Nova can't join it). The lock shows the
/// rest.
fn security_label(security: WifiSecurity) -> Option<&'static str> {
    match security {
        WifiSecurity::Open => Some("Open"),
        WifiSecurity::Wep => Some("WEP"),
        WifiSecurity::Enterprise => Some("Enterprise"),
        WifiSecurity::Wpa | WifiSecurity::Wpa2 | WifiSecurity::Wpa3 => None,
    }
}

fn band_label(band: WifiBand) -> &'static str {
    match band {
        WifiBand::Ghz2_4 => "2.4 GHz",
        WifiBand::Ghz5 => "5 GHz",
        WifiBand::Ghz6 => "6 GHz",
    }
}

/// Nova connects to it: not WEP, and not 802.1X (set up in the system's
/// network settings).
pub fn joinable(net: &WifiNetwork) -> bool {
    !matches!(net.security, WifiSecurity::Wep | WifiSecurity::Enterprise)
}

/// Clicking it asks for a password first: secured and not saved.
pub fn needs_password(net: &WifiNetwork) -> bool {
    joinable(net) && !net.saved && net.security != WifiSecurity::Open
}

/// The last connect to `ssid` failed for another reason than its password.
pub fn failed(network: Option<&Network>, ssid: &str) -> bool {
    network
        .and_then(|network| network.wifi.as_ref()?.attempt.as_ref())
        .is_some_and(|attempt| attempt.ssid == ssid && attempt.outcome == AttemptOutcome::Failed)
}

/// A network row's second line: "Saved · 2.4 GHz", or how a connect to it
/// is going.
pub fn network_line(network: Option<&Network>, net: &WifiNetwork) -> String {
    if connecting(network) == Some(net.ssid.as_str()) {
        return "Connecting…".into();
    }
    if failed(network, &net.ssid) {
        return "Couldn't connect".into();
    }
    let saved = (net.saved && !net.connected).then_some("Saved");
    let parts = [saved, security_label(net.security), net.band.map(band_label)];
    parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// Signal strengths (0 to 100) from which Wi-Fi's icon lights two and
/// three of its arcs.
const TWO_ARCS_FROM: u8 = 34;
const THREE_ARCS_FROM: u8 = 67;

/// How many of Wi-Fi's three arcs a signal of `strength` lights.
pub fn arcs(strength: u8) -> usize {
    match strength {
        THREE_ARCS_FROM.. => 3,
        TWO_ARCS_FROM.. => 2,
        _ => 1,
    }
}

/// The network it's on, listed above the others.
pub fn current(network: Option<&Network>) -> Option<&WifiNetwork> {
    if !wifi_up(network) {
        return None;
    }
    network?.wifi.as_ref()?.networks.iter().find(|net| net.connected)
}

/// The rest of the list, in novad's order.
pub fn others(network: Option<&Network>) -> Vec<&WifiNetwork> {
    if !wifi_on(network) {
        return Vec::new();
    }
    let on = current(network).map(|net| net.ssid.as_str());
    let networks = network.and_then(|network| network.wifi.as_ref()).map_or(&[][..], |wifi| wifi.networks.as_slice());
    networks.iter().filter(|net| !(net.connected && Some(net.ssid.as_str()) == on)).collect()
}

/// Connect can be pressed: novad takes a password this long (UTF-8 bytes).
pub fn password_fits(text: &str) -> bool {
    (WPA_PASSWORD_MIN_BYTES..=WIFI_PASSWORD_MAX_BYTES).contains(&text.len())
}

/// Bytes per second as "3.5 MB/s", "114 KB/s".
pub fn speed(bytes_per_second: u64) -> String {
    format!("{}/s", nova_look::bytes::bytes(bytes_per_second))
}

/// The newest second of a history, or 0.
pub fn now(values: &[u64]) -> u64 {
    values.last().copied().unwrap_or(0)
}

/// The fullest second in either history, so both lines share one scale.
pub fn peak(traffic: &NetworkTraffic) -> u64 {
    traffic.down.iter().chain(&traffic.up).copied().max().unwrap_or(0)
}

/// A traffic history as shares of `peak` (0 to 100), for the graph.
pub fn scaled(values: &[u64], peak: u64) -> Vec<f32> {
    values
        .iter()
        .map(|value| match peak {
            0 => 0.0,
            peak => (*value as f64 / peak as f64 * f64::from(nova_look::graph::FULL)) as f32,
        })
        .collect()
}

/// "today ↓ 1.2 GB ↑ 80 MB".
pub fn today(traffic: &NetworkTraffic) -> String {
    format!("today ↓ {} ↑ {}", nova_look::bytes::bytes(traffic.today_down), nova_look::bytes::bytes(traffic.today_up))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_network_api::{Wifi, WifiAttempt};

    fn net(ssid: &str, strength: u8, security: WifiSecurity, saved: bool, connected: bool) -> WifiNetwork {
        WifiNetwork { ssid: ssid.into(), strength, security, band: Some(WifiBand::Ghz5), saved, connected }
    }

    fn wifi(state: LinkState, networks: Vec<WifiNetwork>) -> Wifi {
        Wifi {
            interface: "wlp6s0".into(),
            enabled: true,
            blocked: false,
            state,
            network: networks.iter().find(|net| net.connected).map(|net| net.ssid.clone()),
            speed_mbps: Some(585),
            scanning: false,
            networks,
            attempt: None,
        }
    }

    fn cable(state: LinkState) -> WiredLink {
        WiredLink { interface: "enp5s0".into(), state, connection: None, speed_mbps: Some(1000) }
    }

    fn home() -> Network {
        Network {
            running: true,
            primary: Some(LinkKind::Wifi),
            prefer: None,
            wired: vec![],
            wifi: Some(wifi(
                LinkState::Connected,
                vec![
                    net("Home", 84, WifiSecurity::Wpa2, true, true),
                    net("Cafe", 31, WifiSecurity::Open, false, false),
                    net("Office", 50, WifiSecurity::Wpa2, false, false),
                ],
            )),
        }
    }

    #[test]
    fn a_connect_under_way_comes_from_the_attempt_or_the_card() {
        let mut network = home();
        assert_eq!(connecting(Some(&network)), None);
        let wifi = network.wifi.as_mut().unwrap();
        wifi.attempt = Some(WifiAttempt { ssid: "Cafe".into(), outcome: AttemptOutcome::Connecting });
        assert_eq!(connecting(Some(&network)), Some("Cafe"));
        let wifi = network.wifi.as_mut().unwrap();
        wifi.attempt = None;
        wifi.state = LinkState::Connecting;
        assert_eq!(connecting(Some(&network)), Some("Home"));
        assert_eq!(connecting(None), None);
    }

    #[test]
    fn the_wired_row_shows_the_connected_port_else_one_with_a_cable() {
        let mut network = home();
        assert!(wired_link(Some(&network)).is_none());
        network.wired = vec![cable(LinkState::Unavailable), cable(LinkState::Disconnected)];
        assert_eq!(wired_link(Some(&network)).map(|link| link.state), Some(LinkState::Disconnected));
        network.wired.push(cable(LinkState::Connected));
        assert_eq!(wired_link(Some(&network)).map(|link| link.state), Some(LinkState::Connected));
    }

    #[test]
    fn roles_say_which_link_carries_traffic() {
        let mut network = home();
        assert_eq!(role(Some(&network), LinkKind::Wifi), Role::Connected, "Wi-Fi alone");
        network.wired = vec![cable(LinkState::Connected)];
        assert_eq!(role(Some(&network), LinkKind::Wifi), Role::Use);
        assert_eq!(role(Some(&network), LinkKind::Wired), Role::Backup);
        network.primary = Some(LinkKind::Wired);
        assert_eq!(role(Some(&network), LinkKind::Wired), Role::Use);
        assert!(prefer_shown(Some(&network)));
        network.wired = vec![cable(LinkState::Disconnected)];
        assert_eq!(role(Some(&network), LinkKind::Wifi), Role::Use, "beside a Wired row");
        assert!(!prefer_shown(Some(&network)));
    }

    #[test]
    fn the_tile_shows_the_link_in_use() {
        let mut network = home();
        assert_eq!(tile_icon(Some(&network)), LinkIcon { glyph: Glyph::WIFI, fill: Some(Glyph::WIFI_DOT) });
        assert!(tile_on(Some(&network)));
        network.primary = Some(LinkKind::Wired);
        assert_eq!(tile_icon(Some(&network)).glyph, Glyph::WIRED);
        network.primary = None;
        assert_eq!(tile_icon(Some(&network)), LinkIcon { glyph: Glyph::WIFI_OFF, fill: Some(Glyph::WIFI_DOT) });
        assert!(!tile_on(Some(&network)));
        network.wifi = None;
        assert_eq!(tile_icon(Some(&network)), LinkIcon { glyph: Glyph::WIRED_OFF, fill: None });
        assert_eq!(tile_icon(None).glyph, Glyph::WIRED_OFF);
    }

    #[test]
    fn the_card_names_the_link_and_its_speed() {
        let mut network = home();
        assert_eq!(link_name(Some(&network)), "Home");
        assert_eq!(link_mbps(Some(&network)), 585);
        network.primary = Some(LinkKind::Wired);
        network.wired = vec![cable(LinkState::Connected)];
        assert_eq!(link_name(Some(&network)), "Wired");
        assert_eq!(link_mbps(Some(&network)), 1000);
        network.primary = None;
        assert_eq!(link_name(Some(&network)), "Not connected");
        assert_eq!(link_mbps(Some(&network)), 0);
        assert_eq!(link_speed(1000), "1 Gb/s");
        assert_eq!(link_speed(2500), "2.5 Gb/s");
        assert_eq!(link_speed(866), "866 Mb/s");
        assert_eq!(wired_line(&cable(LinkState::Connected)), "enp5s0 · 1 Gb/s");
    }

    #[test]
    fn a_row_names_what_changes_a_click_and_how_a_connect_goes() {
        let mut network = home();
        let cafe = net("Cafe", 31, WifiSecurity::Open, false, false);
        assert_eq!(network_line(Some(&network), &cafe), "Open · 5 GHz");
        let saved = net("Flat", 40, WifiSecurity::Wpa2, true, false);
        assert_eq!(network_line(Some(&network), &saved), "Saved · 5 GHz");
        let on = net("Home", 84, WifiSecurity::Wpa2, true, true);
        assert_eq!(network_line(Some(&network), &on), "5 GHz");
        let wifi = network.wifi.as_mut().unwrap();
        wifi.attempt = Some(WifiAttempt { ssid: "Cafe".into(), outcome: AttemptOutcome::Connecting });
        assert_eq!(network_line(Some(&network), &cafe), "Connecting…");
        let wifi = network.wifi.as_mut().unwrap();
        wifi.attempt = Some(WifiAttempt { ssid: "Cafe".into(), outcome: AttemptOutcome::Failed });
        assert_eq!(network_line(Some(&network), &cafe), "Couldn't connect");
        assert!(failed(Some(&network), "Cafe"));
    }

    #[test]
    fn what_a_click_on_a_network_does() {
        assert!(needs_password(&net("Office", 50, WifiSecurity::Wpa2, false, false)));
        assert!(!needs_password(&net("Flat", 50, WifiSecurity::Wpa2, true, false)), "saved");
        assert!(!needs_password(&net("Cafe", 50, WifiSecurity::Open, false, false)));
        assert!(!joinable(&net("Old", 50, WifiSecurity::Wep, false, false)));
        assert!(!joinable(&net("Work", 50, WifiSecurity::Enterprise, false, false)));
        assert!(!needs_password(&net("Work", 50, WifiSecurity::Enterprise, false, false)));
    }

    #[test]
    fn the_list_puts_the_network_it_is_on_first() {
        let mut network = home();
        assert_eq!(current(Some(&network)).map(|net| net.ssid.as_str()), Some("Home"));
        let names = |network: &Network| others(Some(network)).iter().map(|net| net.ssid.clone()).collect::<Vec<_>>();
        assert_eq!(names(&network), ["Cafe", "Office"]);
        network.wifi.as_mut().unwrap().enabled = false;
        assert!(names(&network).is_empty());
        assert!(current(Some(&network)).is_none());
    }

    #[test]
    fn signal_lights_arcs() {
        assert_eq!(arcs(0), 1);
        assert_eq!(arcs(33), 1);
        assert_eq!(arcs(34), 2);
        assert_eq!(arcs(66), 2);
        assert_eq!(arcs(67), 3);
        assert_eq!(arcs(100), 3);
    }

    #[test]
    fn a_password_fits_by_its_bytes() {
        assert!(!password_fits("1234567"));
        assert!(password_fits("12345678"));
        assert!(password_fits("ééééé"), "ten bytes in five characters");
        assert!(password_fits(&"a".repeat(128)));
        assert!(!password_fits(&"a".repeat(129)));
    }

    #[test]
    fn traffic_reads_per_second_on_one_scale() {
        let traffic =
            NetworkTraffic { down: vec![0, 2_048, 4_096], up: vec![1_024], today_down: 2_576_980_378, today_up: 0 };
        assert_eq!(peak(&traffic), 4_096);
        assert_eq!(scaled(&traffic.down, 4_096), [0.0, 50.0, 100.0]);
        assert_eq!(scaled(&traffic.up, 0), [0.0]);
        assert_eq!(speed(now(&traffic.down)), "4.0 KB/s");
        assert_eq!(speed(now(&[])), "0 B/s");
        assert_eq!(today(&traffic), "today ↓ 2.4 GB ↑ 0 B");
    }
}
