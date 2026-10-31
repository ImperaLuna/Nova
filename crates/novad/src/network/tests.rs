//! The network service against a stand-in for NetworkManager only: a
//! private `dbus-daemon` per test with `fake_nm` serving what RyzenShine's
//! NetworkManager reported (`recorded/nm-ryzenshine.json`: a desktop on a
//! cable and Wi-Fi at once, Wi-Fi carrying traffic by its profile's route
//! metric, plus loopback, a libvirt bridge, Wi-Fi P2P and Tailscale's
//! tunnel), and the signals of a rescan there (`recorded/nm-rescan.jsonl`,
//! which found one more network). Network names other than the user's are
//! `Neighbor-N`, the user's own were renamed, and MAC addresses and IP
//! addresses were scrubbed.

use super::fake_nm::FakeNm;
use super::*;
use crate::hub::{ClientQueue, Outgoing};
use crate::session_bus::TestBus;
use nova_protocol::{LogLevel, Topic};
use nova_test_support::TempDir;
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;

/// How long a test waits for what should come at once: a read after the
/// settle, a connection attempt.
const WAIT: Duration = Duration::from_secs(15);
const WIRED_DEVICE: &str = "/org/freedesktop/NetworkManager/Devices/2";

fn recorded(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/network/recorded").join(name)).unwrap()
}

struct Fixture {
    dir: TempDir,
    bus: TestBus,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    client: ClientId,
    _ui_stream: UnixStream,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("network-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let client = hub.register(queue);
        hub.subscribe(client, Topic::Network);
        Self { dir, bus, hub, logger, ui, client, _ui_stream: ui_stream }
    }

    /// NetworkManager as RyzenShine's was.
    fn nm(&self) -> FakeNm {
        self.nm_with(|_| {})
    }

    /// NetworkManager as RyzenShine's was, after `edit` changes the
    /// recording.
    fn nm_with(&self, edit: impl FnOnce(&mut Value)) -> FakeNm {
        let mut recording = serde_json::from_str(&recorded("nm-ryzenshine.json")).unwrap();
        edit(&mut recording);
        // A connection of its own, without zbus's object server: the fake
        // answers every call itself.
        let connection = zbus::block_on(zbus::connection::Builder::address(self.bus.address.as_str()).build()).unwrap();
        FakeNm::start(connection, &recording)
    }

    fn start(&self) -> Arc<Network> {
        let sources = Sources {
            bus: Bus::Address(self.bus.address.clone()),
            sys_root: self.dir.join("sys"),
            state_dir: self.dir.join("state"),
        };
        let network = Network::start(sources, Arc::clone(&self.hub), Arc::clone(&self.logger));
        // A new run first clears what clients heard from an earlier one.
        assert_eq!(self.next()["running"], json!(false));
        network
    }

    fn next(&self) -> Value {
        crate::hub::next_named(&self.ui, "network", WAIT)
    }

    /// The next network event that `wanted` holds for, skipping others.
    fn until(&self, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no network event with {what}");
            let event = crate::hub::next_named(&self.ui, "network", left);
            if wanted(&event) {
                return event;
            }
        }
    }

    /// Kernel counters for a link, as `/sys/class/net` has them.
    fn counters(&self, link: &str, down: u64, up: u64) {
        let statistics = self.dir.join("sys/class/net").join(link).join("statistics");
        std::fs::create_dir_all(&statistics).unwrap();
        crate::state_file::write(&statistics.join("rx_bytes"), down.to_string().as_bytes()).unwrap();
        crate::state_file::write(&statistics.join("tx_bytes"), up.to_string().as_bytes()).unwrap();
    }
}

fn network_names(event: &Value) -> Vec<&str> {
    event["wifi"]["networks"].as_array().unwrap().iter().map(|network| network["ssid"].as_str().unwrap()).collect()
}

#[test]
fn ryzenshine_reads_as_its_cable_and_wifi() {
    let fixture = Fixture::new("read");
    let _nm = fixture.nm();
    let network = fixture.start();
    assert_eq!(
        fixture.next(),
        json!({
            "running": true,
            "primary": "wifi",
            "prefer": "wifi",
            "wired": [{"interface": "enp5s0", "state": "connected", "connection": "Wired connection 1", "speed_mbps": 1000}],
            "wifi": {
                "interface": "wlp6s0", "enabled": true, "blocked": false, "state": "connected", "network": "Home",
                "speed_mbps": 585, "scanning": false,
                "networks": [
                    {"ssid": "Home", "strength": 69, "security": "wpa2", "band": "5ghz", "saved": true, "connected": true},
                ],
            },
        }),
        "the bridge, loopback, Wi-Fi P2P and the tunnel aren't links; access points hiding their name aren't listed; \
         the network shows as the access point it's on, not its strongest"
    );
    network.stop();
}

#[test]
fn a_rescan_lists_the_network_it_found_once_its_signals_settle() {
    let fixture = Fixture::new("rescan");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();
    for line in recorded("nm-rescan.jsonl").lines() {
        nm.replay(&serde_json::from_str(line).unwrap());
    }
    let event = fixture.next();
    assert_eq!(network_names(&event), ["Home", "Neighbor-1"], "one read for the whole burst: {event}");
    assert_eq!(
        event["wifi"]["networks"][1],
        json!({"ssid": "Neighbor-1", "strength": 12, "security": "wpa2", "band": "2.4ghz", "saved": false, "connected": false})
    );
    network.stop();
}

#[test]
fn a_new_network_takes_its_password_and_a_wrong_one_isnt_kept() {
    let fixture = Fixture::new("password");
    let nm = fixture.nm();
    nm.set_password("Neighbor-1", "the right one");
    let network = fixture.start();
    fixture.next();
    for line in recorded("nm-rescan.jsonl").lines() {
        nm.replay(&serde_json::from_str(line).unwrap());
    }
    fixture.until("Neighbor-1", |event| network_names(event).contains(&"Neighbor-1"));

    assert_eq!(network.connect("Neighbor-1", None), Err("Neighbor-1 needs a password".into()));
    assert!(network.connect("Neighbor-1", Some(&WifiPassword::new("short"))).unwrap_err().contains("at least 8"));

    network.connect("Neighbor-1", Some(&WifiPassword::new("the wrong one"))).unwrap();
    let event = fixture.until("a wrong password", |event| event["wifi"]["attempt"]["outcome"] == "wrong_password");
    assert_eq!(event["wifi"]["attempt"]["ssid"], "Neighbor-1");
    nova_test_support::wait_until(WAIT, "the profile it made is removed", || {
        !nm.profile_ids().contains(&"Neighbor-1".to_string())
    });
    let event = fixture.until("Neighbor-1 not saved", |event| {
        event["wifi"]["networks"][1]["ssid"] == "Neighbor-1" && event["wifi"]["networks"][1]["saved"] == false
    });
    assert_eq!(event["wifi"]["network"], "Home", "still on the network it was on");

    network.connect("Neighbor-1", Some(&WifiPassword::new("the right one"))).unwrap();
    let event = fixture.until("connected", |event| event["wifi"]["network"] == "Neighbor-1");
    assert_eq!(event["wifi"]["attempt"], json!({"ssid": "Neighbor-1", "outcome": "connected"}));
    assert_eq!(event["wifi"]["networks"][0]["ssid"], "Neighbor-1", "the network it's on comes first");
    assert_eq!(event["wifi"]["networks"][0]["saved"], true);
    let log = std::fs::read_to_string(fixture.dir.join("logs/nova.log")).unwrap_or_default();
    assert!(!log.contains("the right one") && !log.contains("the wrong one"), "passwords never reach the log");
    network.stop();
}

#[test]
fn a_saved_network_connects_again_without_a_password_and_can_be_forgotten() {
    let fixture = Fixture::new("saved");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();

    network.disconnect().unwrap();
    let event = fixture.until("disconnected", |event| event["wifi"]["state"] == "disconnected");
    assert_eq!(event["primary"], "wired", "the cable carries traffic once Wi-Fi is off it");
    assert!(event["wifi"].get("network").is_none());

    network.connect("Home", None).unwrap();
    let event = fixture.until("connected", |event| event["wifi"]["state"] == "connected");
    assert_eq!(event["wifi"]["network"], "Home");
    assert_eq!(event["primary"], "wifi", "its profile's route metric puts it first again");
    assert_eq!(nm.calls("AddAndActivateConnection"), Vec::<String>::new(), "its saved profile was used");

    network.forget("Home").unwrap();
    fixture.until("Home forgotten", |event| event["wifi"]["networks"][0]["saved"] == false);
    assert!(!nm.profile_ids().contains(&"Home".to_string()));
    assert_eq!(network.forget("Home"), Err("Home isn't saved".into()));
    network.stop();
}

#[test]
fn preferring_a_link_sets_route_metrics_on_the_devices_not_the_profiles() {
    let fixture = Fixture::new("prefer");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();

    // The profiles put Wi-Fi first (metric 10 against the cable's 900).
    network.prefer(LinkKind::Wired).unwrap();
    let event = fixture.until("the cable in use", |event| event["primary"] == "wired");
    assert_eq!(event["prefer"], "wired");
    assert_eq!(nm.calls("Reapply"), [WIRED_DEVICE], "the cable goes just under Wi-Fi's 10; Wi-Fi keeps its own");
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.dir.join("state/network.json")).unwrap()).unwrap();
    assert_eq!(saved, json!({"prefer": "wired"}));

    network.prefer(LinkKind::Wifi).unwrap();
    let event = fixture.until("Wi-Fi in use", |event| event["primary"] == "wifi");
    assert_eq!(event["prefer"], "wifi");
    assert_eq!(nm.calls("Reapply"), [WIRED_DEVICE, WIRED_DEVICE], "the cable gets its profile's metric back");
    assert_eq!(network.prefer(LinkKind::Other), Err("only a wired or a Wi-Fi link can be preferred".into()));
    network.stop();

    // The pick is kept across restarts.
    let fixture2 = Fixture::new("prefer-again");
    std::fs::create_dir_all(fixture2.dir.join("state")).unwrap();
    std::fs::copy(fixture.dir.join("state/network.json"), fixture2.dir.join("state/network.json")).unwrap();
    let _nm = fixture2.nm();
    let network = fixture2.start();
    assert_eq!(fixture2.next()["prefer"], "wifi");
    network.stop();
}

#[test]
fn a_preference_that_failed_to_apply_is_tried_again() {
    let fixture = Fixture::new("prefer-retry");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();

    nm.fail_next("Reapply");
    network.prefer(LinkKind::Wired).unwrap();
    fixture.until("the cable in use", |event| event["primary"] == "wired");
    assert_eq!(nm.calls("Reapply"), [WIRED_DEVICE, WIRED_DEVICE], "the first failed, the second took");
    network.stop();
}

#[test]
fn a_preference_holds_for_ipv6_where_its_metrics_differ() {
    let fixture = Fixture::new("prefer-ipv6");
    // Wi-Fi first for IPv4 (10 against 900), the cable first for IPv6.
    let wired_profile = "/org/freedesktop/NetworkManager/Settings/3";
    let nm =
        fixture.nm_with(|recording| {
            recording[wired_profile][manager::SETTINGS_CONNECTION]["GetSettings"]["data"][0]["ipv6"]["route-metric"]
                ["data"] = json!(5);
        });
    let network = fixture.start();
    fixture.next();

    // Already Wi-Fi's for IPv4, so the event doesn't change.
    network.prefer(LinkKind::Wifi).unwrap();
    let wifi_device = "/org/freedesktop/NetworkManager/Devices/3";
    nova_test_support::wait_until(WAIT, "a reapply", || !nm.calls("Reapply").is_empty());
    assert_eq!(nm.calls("Reapply"), [wifi_device], "only Wi-Fi's IPv6 needed to change");
    assert_eq!(
        nm.applied_metrics(wifi_device),
        Some([10, 4]),
        "IPv4 keeps its profile's 10, IPv6 goes under the cable's 5"
    );
    network.stop();
}

#[test]
fn wifi_turns_off_and_scans() {
    let fixture = Fixture::new("switch");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();

    network.scan().unwrap();
    fixture.until("scanning", |event| event["wifi"]["scanning"] == true);
    fixture.until("the scan done", |event| event["wifi"]["scanning"] == false);

    network.set_wifi(false).unwrap();
    fixture.until("Wi-Fi off", |event| event["wifi"]["enabled"] == false);
    assert_eq!(nm.property("/org/freedesktop/NetworkManager", NM, "WirelessEnabled"), json!(false));

    nm.change("/org/freedesktop/NetworkManager", NM, &[("WirelessHardwareEnabled", "b", json!(false))]);
    fixture.until("blocked", |event| event["wifi"]["blocked"] == true);
    assert_eq!(network.set_wifi(true), Err("Wi-Fi is turned off by a hardware switch".into()));
    network.stop();
}

#[test]
fn without_networkmanager_nothing_runs_and_commands_say_why() {
    let fixture = Fixture::new("none");
    let network = fixture.start();
    assert_eq!(network.scan(), Err("NetworkManager isn't running".into()));
    assert_eq!(network.connect("Home", None), Err("NetworkManager isn't running".into()));
    assert_eq!(network.connect("", None), Err("a network name can't be empty".into()));
    network.stop();
}

#[test]
fn an_attempt_keeps_its_outcome_through_other_connections_changing() {
    let fixture = Fixture::new("churn");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();
    network.disconnect().unwrap();
    fixture.until("Wi-Fi off its network", |event| event["wifi"].get("network").is_none());
    network.connect("Home", None).unwrap();
    fixture.until("connected", |event| event["wifi"]["attempt"]["outcome"] == "connected");

    // A VPN or the cable going up and down, many times over.
    for _ in 0..=MAX_TRANSITIONS {
        nm.state_changed("/org/freedesktop/NetworkManager/ActiveConnection/1", ACTIVE_STATE_ACTIVATED, 0);
    }
    network.scan().unwrap();
    let event = fixture.until("a read after them", |event| event["wifi"]["scanning"] == true);
    assert_eq!(event["wifi"]["attempt"]["outcome"], "connected");
    network.stop();
}

#[test]
fn an_attempt_ends_when_networkmanager_goes() {
    let fixture = Fixture::new("gone");
    let nm = fixture.nm();
    let network = fixture.start();
    fixture.next();
    network.disconnect().unwrap();
    fixture.until("Wi-Fi off its network", |event| event["wifi"].get("network").is_none());

    nm.stall();
    network.connect("Home", None).unwrap();
    fixture.until("connecting", |event| event["wifi"]["attempt"]["outcome"] == "connecting");
    nm.vanish();
    fixture.until("NetworkManager gone", |event| event["running"] == false);
    let _back = fixture.nm();
    let event = fixture.until("NetworkManager back", |event| event["running"] == true);
    assert!(event["wifi"].get("attempt").is_none(), "its last state change won't come: {event}");
    network.stop();
}

#[test]
fn networkmanager_starting_later_is_read() {
    let fixture = Fixture::new("later");
    let network = fixture.start();
    std::thread::sleep(SETTLE * 2);
    let _nm = fixture.nm();
    assert_eq!(fixture.until("running", |event| event["running"] == true)["primary"], "wifi");
    network.stop();
}

#[test]
fn traffic_is_counted_over_the_links_while_watched() {
    let fixture = Fixture::new("traffic");
    fixture.counters("enp5s0", 1_000, 100);
    fixture.counters("wlp6s0", 5_000, 500);
    // Not a link NetworkManager lists: never counted.
    fixture.counters("virbr0", 9_000_000, 9_000_000);
    let _nm = fixture.nm();
    let network = fixture.start();
    fixture.next();
    network.watch_traffic(fixture.client, true);
    // A look once the links are known sets where counting starts: the
    // second event is one (the first may be what was there before them).
    for _ in 0..2 {
        crate::hub::next_named(&fixture.ui, "network_traffic", WAIT);
    }
    fixture.counters("enp5s0", 3_000, 200);
    fixture.counters("wlp6s0", 9_000, 900);
    let deadline = Instant::now() + WAIT;
    let traffic = loop {
        let traffic = crate::hub::next_named(&fixture.ui, "network_traffic", WAIT);
        if traffic["today_down"] != 0 || Instant::now() > deadline {
            break traffic;
        }
    };
    assert_eq!((traffic["today_down"].clone(), traffic["today_up"].clone()), (json!(6_000), json!(500)));
    assert!(traffic["down"].as_array().unwrap().last().unwrap().as_u64().unwrap() > 0);

    network.stop();
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.dir.join("state/network-traffic.json")).unwrap())
            .unwrap();
    assert_eq!((saved["down"].clone(), saved["up"].clone()), (json!(6_000), json!(500)), "saved when it stops");
}
