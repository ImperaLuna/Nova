//! Bluetooth against a stand-in for BlueZ only: a private `dbus-daemon` per
//! test with a fake `org.bluez` that serves an adapter and its devices as
//! BlueZ's D-Bus API describes them (`GetManagedObjects`, the signals,
//! `Pair` calling the registered agent back), and a `/sys/class/rfkill`
//! and `/dev/rfkill` in the test's folder. novad's agent registers on that
//! bus, never on the machine's.
//!
//! The devices are written from BlueZ's documented properties, not
//! recorded: the desktop these tests were written for has Bluetooth
//! blocked.

use super::*;
use bluez::MEDIA_TRANSPORT;
use nova_carrier::log::LogLevel;
use nova_hub::{ClientQueue, Outgoing};
use nova_session_bus::TestBus;
use nova_test_support::{TempDir, wait_until};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use zbus::interface;
use zbus::wire::{OwnedVariant, Str};

mod fake_bluez;
use fake_bluez::*;

/// How long a test waits for what should come at once.
const WAIT: Duration = Duration::from_secs(15);
/// How long A2DP may take to come up here: the fake brings it at once.
const AUDIO_WAIT_HERE: Duration = Duration::from_millis(500);
const ADAPTER_PATH: &str = "/org/bluez/hci0";
const HEADPHONES: &str = "00:1B:66:AA:BB:CC";
const PHONE: &str = "9C:2A:70:11:22:33";
const BUDS: &str = "C4:30:18:44:55:66";
const NAMELESS: &str = "4C:11:22:33:44:55";
const BATTERY: &str = "org.bluez.Battery1";
const A2DP_SINK: &str = "0000110b-0000-1000-8000-00805f9b34fb";
/// LE Audio's published audio capabilities.
const LE_AUDIO: &str = "00001850-0000-1000-8000-00805f9b34fb";

fn device_path(address: &str) -> String {
    format!("{ADAPTER_PATH}/dev_{}", address.replace(':', "_"))
}

fn text(value: &str) -> OwnedVariant {
    OwnedVariant::from(Str::from(value.to_owned()))
}

fn path(value: &str) -> OwnedVariant {
    OwnedVariant::from(ObjectPath::try_from(value.to_owned()).unwrap())
}

/// A device as BlueZ lists it.
fn device(address: &str, name: Option<&str>, icon: &str, paired: bool, connected: bool) -> Interfaces {
    let mut properties: Properties = HashMap::from([
        ("Address".into(), text(address)),
        ("Adapter".into(), path(ADAPTER_PATH)),
        // BlueZ's alias for a nameless device is its address with dashes.
        ("Alias".into(), text(&name.map_or_else(|| address.replace(':', "-"), str::to_owned))),
        ("Icon".into(), text(icon)),
        ("Paired".into(), OwnedVariant::from(paired)),
        ("Trusted".into(), OwnedVariant::from(paired)),
        ("Connected".into(), OwnedVariant::from(connected)),
    ]);
    if let Some(name) = name {
        properties.insert("Name".into(), text(name));
    }
    HashMap::from([(DEVICE.to_owned(), properties)])
}

/// What BlueZ has on a desktop with headphones on, a phone paired and new
/// earbuds and a nameless beacon nearby.
fn objects() -> BTreeMap<String, Interfaces> {
    let adapter: Properties = HashMap::from([
        ("Address".into(), text("D8:3A:DD:00:11:22")),
        ("Powered".into(), OwnedVariant::from(true)),
        ("Discovering".into(), OwnedVariant::from(false)),
    ]);
    let mut headphones = device(HEADPHONES, Some("WH-1000XM5"), "audio-headphones", true, true);
    headphones.insert(BATTERY.into(), HashMap::from([("Percentage".into(), OwnedVariant::from(80u8))]));
    // Heard by a scan, and offering audio over classic Bluetooth and LE.
    let mut buds = device(BUDS, Some("Pixel Buds Pro"), "audio-headphones", false, false);
    let properties = buds.get_mut(DEVICE).unwrap();
    properties.insert("RSSI".into(), OwnedVariant::from(-60i16));
    let uuids = vec![LE_AUDIO.to_owned(), A2DP_SINK.to_owned()];
    properties.insert("UUIDs".into(), OwnedVariant::try_from(zbus::Variant::from(uuids)).unwrap());
    BTreeMap::from([
        (ADAPTER_PATH.to_owned(), HashMap::from([(ADAPTER.to_owned(), adapter)])),
        (device_path(HEADPHONES), headphones),
        (device_path(PHONE), device(PHONE, Some("Pixel 8"), "phone", true, false)),
        (device_path(BUDS), buds),
        (device_path(NAMELESS), device(NAMELESS, None, "", false, false)),
    ])
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
        let dir = TempDir::new(&format!("bluetooth-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let client = hub.register(queue);
        hub.subscribe(client, nova_bluetooth_api::TOPIC);
        let fixture = Self { dir, bus, hub, logger, ui, client, _ui_stream: ui_stream };
        fixture.switch(false, false);
        std::fs::create_dir_all(fixture.dir.join("dev")).unwrap();
        std::fs::write(fixture.rfkill().device, b"").unwrap();
        fixture
    }

    fn rfkill(&self) -> Rfkill {
        Rfkill {
            class: self.dir.join("sys/class/rfkill"),
            device: self.dir.join("dev/rfkill"),
            adapters: self.dir.join("sys/class/bluetooth"),
        }
    }

    /// Sets the Bluetooth radio switch.
    fn switch(&self, soft: bool, hard: bool) {
        let dir = self.rfkill().class.join("rfkill0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("type"), "bluetooth\n").unwrap();
        std::fs::write(dir.join("soft"), format!("{}\n", u8::from(soft))).unwrap();
        std::fs::write(dir.join("hard"), format!("{}\n", u8::from(hard))).unwrap();
    }

    fn bluez(&self) -> FakeBlueZ {
        FakeBlueZ::start(self.bus.connect(), self.rfkill().class.join("rfkill0/soft"))
    }

    fn start(&self) -> Arc<Bluetooth> {
        self.start_with(CONNECT_LIMIT)
    }

    /// Started with connects stopped after `connect_limit`.
    fn start_with(&self, connect_limit: Duration) -> Arc<Bluetooth> {
        let sources = Sources {
            bus: Bus::Address(self.bus.address.clone()),
            rfkill: self.rfkill(),
            connect_limit,
            audio_wait: AUDIO_WAIT_HERE,
        };
        Bluetooth::start(sources, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    /// The next event that `check` accepts, skipping the ones before it.
    fn until(&self, what: &str, check: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no Bluetooth event with {what}");
            let event = nova_hub::next_named(&self.ui, "bluetooth", left);
            if check(&event) {
                return event;
            }
        }
    }
}

/// `address`'s entry in `event`.
fn find<'a>(event: &'a Value, address: &str) -> Option<&'a Value> {
    event["devices"].as_array().unwrap().iter().find(|device| device["address"] == address)
}

fn status(event: &Value, address: &str) -> Option<String> {
    find(event, address).map(|device| device["status"].as_str().unwrap().to_owned())
}

#[test]
fn lists_the_adapter_and_devices_and_follows_them() {
    let fixture = Fixture::new("lists");
    let bluez = fixture.bluez();
    let _bluetooth = fixture.start();
    let event = fixture.until("the devices", |event| event["radio"] == "on");
    assert_eq!(
        event,
        json!({
            "radio": "on",
            "scanning": false,
            "devices": [
                {"address": HEADPHONES, "name": "WH-1000XM5", "kind": "headphones", "status": "connected", "battery": 80},
                {"address": PHONE, "name": "Pixel 8", "kind": "phone", "status": "paired"},
                {"address": BUDS, "name": "Pixel Buds Pro", "kind": "headphones", "status": "nearby"},
            ],
        }),
        "the nameless beacon isn't listed"
    );
    wait_until(WAIT, "the agent registered", || bluez.calls().len() >= 2);
    let agent = format!("{AGENT_PATH} {CAPABILITY}");
    assert_eq!(bluez.calls()[..2], [format!("RegisterAgent {agent}"), format!("RequestDefaultAgent {AGENT_PATH}")]);

    bluez.change(&device_path(PHONE), DEVICE, "Connected", OwnedVariant::from(true));
    let event = fixture.until("the phone connected", |event| status(event, PHONE).as_deref() == Some("connected"));
    assert_eq!(status(&event, HEADPHONES).as_deref(), Some("connected"));
    bluez.remove(&device_path(BUDS));
    fixture.until("the earbuds gone", |event| find(event, BUDS).is_none());
    bluez.change(ADAPTER_PATH, ADAPTER, "Powered", OwnedVariant::from(false));
    let event = fixture.until("the radio off", |event| event["radio"] == "off");
    assert_eq!(event["devices"], json!([]), "devices only show while it's on");
}

#[test]
fn without_bluez_there_is_no_adapter_and_it_comes_when_bluez_starts() {
    let fixture = Fixture::new("missing");
    let _bluetooth = fixture.start();
    fixture.until("no adapter", |event| event["radio"] == "missing");
    let bluez = fixture.bluez();
    fixture.until("the adapter", |event| event["radio"] == "on");
    wait_until(WAIT, "the agent registered", || bluez.calls().iter().any(|call| call.starts_with("RegisterAgent")));
}

#[test]
fn connecting_a_new_device_pairs_trusts_and_connects_it() {
    let fixture = Fixture::new("pair");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    let event = fixture.until("the question", |event| !event["pairing"].is_null());
    assert_eq!(status(&event, BUDS).as_deref(), Some("pairing"));
    let id = event["pairing"]["id"].as_u64().unwrap();
    assert_eq!(
        event["pairing"],
        json!({"id": id, "address": BUDS, "ask": {"kind": "confirm", "code": "123456"}, "incoming": false})
    );
    assert!(bluetooth.answer(id, PairingAnswer::Code("123456".into())).is_err(), "a code doesn't answer a yes/no");
    bluetooth.answer(id, PairingAnswer::Accept).unwrap();
    assert_eq!(connecting.join().unwrap(), Ok(()));
    let event = fixture.until("the earbuds connected", |event| status(event, BUDS).as_deref() == Some("connected"));
    assert!(event["pairing"].is_null());
    let path = device_path(BUDS);
    let calls: Vec<String> =
        bluez.calls().into_iter().filter(|call| call.ends_with(&path) || call.starts_with("Trusted")).collect();
    assert_eq!(calls, [format!("Pair {path}"), format!("Trusted true {path}"), format!("Connect {path}")]);
    assert!(bluetooth.answer(id, PairingAnswer::Accept).is_err(), "the question is over");
}

#[test]
fn turning_down_the_code_leaves_the_device_unpaired() {
    let fixture = Fixture::new("reject");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    let event = fixture.until("the question", |event| !event["pairing"].is_null());
    bluetooth.answer(event["pairing"]["id"].as_u64().unwrap(), PairingAnswer::Reject).unwrap();
    // Nothing to explain: the user said no.
    assert_eq!(connecting.join().unwrap(), Ok(()));
    let event = fixture.until("the earbuds back", |event| status(event, BUDS).as_deref() == Some("nearby"));
    assert_eq!(find(&event, BUDS).unwrap().get("error"), None);
    assert!(!bluez.calls().iter().any(|call| call.starts_with("Connect")));
}

#[test]
fn an_unanswered_question_ends() {
    let fixture = Fixture::new("timeout");
    let _bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    fixture.until("the question", |event| !event["pairing"].is_null());
    let event = fixture.until("the question gone", |event| event["pairing"].is_null());
    assert_eq!(status(&event, BUDS).as_deref(), Some("pairing"), "gone before pairing fails");
    assert_eq!(connecting.join().unwrap(), Ok(()));
    fixture.until("the earbuds back", |event| status(event, BUDS).as_deref() == Some("nearby"));
}

#[test]
fn a_code_to_type_on_the_device_shows_until_it_pairs() {
    let fixture = Fixture::new("show");
    let bluez = fixture.bluez();
    lock(&bluez.fake).pairing = Pairing::Show(42);
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    let event = fixture.until("the code", |event| !event["pairing"].is_null());
    assert_eq!(event["pairing"]["ask"], json!({"kind": "show_passkey", "code": "000042", "typed": 0}));
    let id = event["pairing"]["id"].as_u64().unwrap();
    assert!(bluetooth.answer(id, PairingAnswer::Accept).is_err(), "nothing to answer");
    lock(&bluez.fake).calls.push("typed".into());
    assert_eq!(connecting.join().unwrap(), Ok(()));
    let event = fixture.until("the earbuds connected", |event| status(event, BUDS).as_deref() == Some("connected"));
    assert!(event["pairing"].is_null(), "the code goes once it's paired");
}

#[test]
fn a_device_pairing_by_itself_asks_as_incoming() {
    let fixture = Fixture::new("incoming");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    wait_until(WAIT, "the agent registered", || lock(&bluez.fake).agent.is_some());
    let asking = std::thread::spawn(move || {
        let result = bluez.confirm_from(PHONE, 7);
        (bluez, result)
    });
    let event = fixture.until("the question", |event| !event["pairing"].is_null());
    assert_eq!(event["pairing"]["incoming"], true);
    assert_eq!(event["pairing"]["ask"]["code"], "000007");
    bluetooth.answer(event["pairing"]["id"].as_u64().unwrap(), PairingAnswer::Accept).unwrap();
    let (_bluez, result) = asking.join().unwrap();
    result.unwrap();
    fixture.until("the question gone", |event| event["pairing"].is_null());
}

#[test]
fn scanning_runs_while_someone_watches() {
    let fixture = Fixture::new("scan");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    bluetooth.watch(fixture.client, true);
    fixture.until("scanning", |event| event["scanning"] == true);
    bluetooth.watch(fixture.client, false);
    fixture.until("no scanning", |event| event["scanning"] == false);
    let scans: Vec<String> = bluez.calls().into_iter().filter(|call| call.ends_with("Discovery")).collect();
    assert_eq!(scans, ["StartDiscovery", "StopDiscovery"]);
}

#[test]
fn scanning_tries_again_when_the_adapter_isnt_ready() {
    let fixture = Fixture::new("scan-retry");
    let bluez = fixture.bluez();
    lock(&bluez.fake).refuse_discovery = 1;
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    bluetooth.watch(fixture.client, true);
    fixture.until("scanning", |event| event["scanning"] == true);
    let starts = bluez.calls().into_iter().filter(|call| call == "StartDiscovery").count();
    assert_eq!(starts, 2, "refused once, then asked again");
}

/// Scanning pauses while a device connects, and goes on after.
#[test]
fn scanning_pauses_while_a_device_connects() {
    let fixture = Fixture::new("scan-connect");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    bluetooth.watch(fixture.client, true);
    fixture.until("scanning", |event| event["scanning"] == true);
    let connecting = held_connect(&fixture, &bluez, &bluetooth, PHONE);
    fixture.until("scanning paused", |event| event["scanning"] == false);
    bluetooth.disconnect(PHONE).unwrap();
    connecting.join().unwrap().unwrap();
    fixture.until("scanning again", |event| event["scanning"] == true);
    let scans: Vec<String> = bluez.calls().into_iter().filter(|call| call.ends_with("Discovery")).collect();
    assert_eq!(scans, ["StartDiscovery", "StopDiscovery", "StartDiscovery"]);
}

#[test]
fn turning_on_lifts_the_software_block() {
    let fixture = Fixture::new("unblock");
    fixture.switch(true, false);
    let bluez = fixture.bluez();
    bluez.change(ADAPTER_PATH, ADAPTER, "Powered", OwnedVariant::from(false));
    let bluetooth = fixture.start();
    fixture.until("the radio off", |event| event["radio"] == "off");
    // The kernel lifts the block once it's asked, a moment later.
    let rfkill = fixture.rfkill();
    let kernel = std::thread::spawn(move || {
        wait_until(WAIT, "the unblock event", || std::fs::read(&rfkill.device).is_ok_and(|event| !event.is_empty()));
        nova_test_support::settle(POWER_RETRY * 3);
        std::fs::write(rfkill.class.join("rfkill0/soft"), "0\n").unwrap();
    });
    bluetooth.power(true).unwrap();
    kernel.join().unwrap();
    assert_eq!(std::fs::read(fixture.rfkill().device).unwrap(), [0, 0, 0, 0, 2, 3, 0, 0]);
    fixture.until("the radio on", |event| event["radio"] == "on");
    assert!(bluez.calls().iter().filter(|call| *call == "Powered true").count() > 1, "asked again until unblocked");
}

#[test]
fn a_hardware_switch_is_left_to_the_user() {
    let fixture = Fixture::new("switch");
    fixture.switch(false, true);
    let _bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the switch", |event| event["radio"] == "switched_off");
    assert!(bluetooth.power(true).unwrap_err().contains("switch"));
    assert!(std::fs::read(fixture.rfkill().device).unwrap().is_empty());
}

#[test]
fn disconnecting_and_forgetting() {
    let fixture = Fixture::new("forget");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    bluetooth.disconnect(HEADPHONES).unwrap();
    fixture.until("the headphones off", |event| status(event, HEADPHONES).as_deref() == Some("paired"));
    bluetooth.forget(PHONE).unwrap();
    fixture.until("the phone gone", |event| find(event, PHONE).is_none());
    assert!(bluez.calls().contains(&format!("RemoveDevice {}", device_path(PHONE))));
    assert!(bluetooth.connect("not an address").unwrap_err().contains("isn't a Bluetooth address"));
    assert!(bluetooth.connect("AA:AA:AA:AA:AA:AA").is_err(), "a device BlueZ doesn't have");
}

#[test]
fn new_devices_show_only_while_a_scan_hears_them() {
    let fixture = Fixture::new("heard");
    let bluez = fixture.bluez();
    let _bluetooth = fixture.start();
    fixture.until("the earbuds", |event| status(event, BUDS).as_deref() == Some("nearby"));
    // Out of range or the scan ended: BlueZ takes `RSSI` back but keeps
    // the device a while.
    bluez.invalidate(&device_path(BUDS), DEVICE, "RSSI");
    let event = fixture.until("the earbuds gone", |event| find(event, BUDS).is_none());
    assert_eq!(status(&event, PHONE).as_deref(), Some("paired"), "paired ones stay");
    bluez.change(&device_path(BUDS), DEVICE, "RSSI", OwnedVariant::from(-70i16));
    fixture.until("the earbuds back", |event| status(event, BUDS).as_deref() == Some("nearby"));
}

#[test]
fn an_aborted_le_connect_connects_over_classic_bluetooth() {
    let fixture = Fixture::new("le-abort");
    let bluez = fixture.bluez();
    lock(&bluez.fake).abort_le = true;
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    let event = fixture.until("the question", |event| !event["pairing"].is_null());
    bluetooth.answer(event["pairing"]["id"].as_u64().unwrap(), PairingAnswer::Accept).unwrap();
    assert_eq!(connecting.join().unwrap(), Ok(()));
    fixture.until("the earbuds connected", |event| status(event, BUDS).as_deref() == Some("connected"));
    let path = device_path(BUDS);
    let calls: Vec<String> = bluez.calls().into_iter().filter(|call| call.contains("Connect")).collect();
    assert_eq!(calls, [format!("Connect {path}"), format!("ConnectProfile {A2DP_SINK} {path}")]);

    // A device with no classic profile keeps BlueZ's failure.
    assert_eq!(bluetooth.connect(PHONE).unwrap_err(), "Couldn't connect");
}

/// Starts connecting `address` on a thread: the fake holds it until it's
/// cancelled.
fn held_connect(
    fixture: &Fixture,
    bluez: &FakeBlueZ,
    bluetooth: &Arc<Bluetooth>,
    address: &'static str,
) -> std::thread::JoinHandle<Result<(), String>> {
    lock(&bluez.fake).held = Some(Held { step: Hold::Connect, waiting: false, cancelled: false, waker: None });
    let connecting = {
        let bluetooth = Arc::clone(bluetooth);
        std::thread::spawn(move || bluetooth.connect(address))
    };
    fixture.until("connecting", |event| status(event, address).as_deref() == Some("connecting"));
    wait_until(WAIT, "the connect under way", || lock(&bluez.fake).held.as_ref().is_some_and(|held| held.waiting));
    connecting
}

#[test]
fn a_connect_under_way_can_be_stopped() {
    let fixture = Fixture::new("stop");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = held_connect(&fixture, &bluez, &bluetooth, PHONE);
    bluetooth.disconnect(PHONE).unwrap();
    assert_eq!(connecting.join().unwrap(), Ok(()), "stopped: nothing to explain");
    let event = fixture.until("the phone back", |event| status(event, PHONE).as_deref() == Some("paired"));
    assert_eq!(find(&event, PHONE).unwrap().get("error"), None);
}

/// A device that never answers (off, or a headset switched to its dongle)
/// doesn't keep the row connecting: the connect stops at its limit and
/// says why.
#[test]
fn a_connect_that_runs_too_long_stops_and_says_so() {
    let fixture = Fixture::new("connect-limit");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start_with(Duration::from_millis(300));
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = held_connect(&fixture, &bluez, &bluetooth, PHONE);
    let failure = connecting.join().unwrap().unwrap_err();
    assert_eq!(failure, "No answer");
    assert!(bluez.calls().contains(&format!("Disconnect {}", device_path(PHONE))), "BlueZ stops trying");
    let event = fixture.until("the phone back", |event| status(event, PHONE).as_deref() == Some("paired"));
    assert_eq!(find(&event, PHONE).unwrap()["error"].as_str(), Some(failure.as_str()));
}

#[test]
fn turning_off_stops_a_connect_first() {
    let fixture = Fixture::new("off-while-connecting");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = held_connect(&fixture, &bluez, &bluetooth, PHONE);
    bluetooth.power(false).unwrap();
    assert_eq!(connecting.join().unwrap(), Ok(()));
    fixture.until("the radio off", |event| event["radio"] == "off");
    let calls: Vec<String> = bluez
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("Disconnect") || call.starts_with("Powered"))
        .collect();
    assert_eq!(calls, [format!("Disconnect {}", device_path(PHONE)), "Powered false".to_owned()]);
}

/// Stopped between pairing and connecting, when no call is under way to
/// cancel: it doesn't go on to connect.
#[test]
fn a_stop_between_steps_holds() {
    let fixture = Fixture::new("stop-between");
    let bluez = fixture.bluez();
    lock(&bluez.fake).held = Some(Held { step: Hold::Trust, waiting: false, cancelled: false, waker: None });
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    let event = fixture.until("the question", |event| !event["pairing"].is_null());
    bluetooth.answer(event["pairing"]["id"].as_u64().unwrap(), PairingAnswer::Accept).unwrap();
    wait_until(WAIT, "trusting it", || lock(&bluez.fake).held.as_ref().is_some_and(|held| held.waiting));
    bluetooth.disconnect(BUDS).unwrap();
    assert_eq!(connecting.join().unwrap(), Ok(()), "stopped: nothing to explain");
    assert!(!bluez.calls().iter().any(|call| call.starts_with("Connect")), "{:?}", bluez.calls());
}

/// Starts connecting the new earbuds on a thread and accepts the pairing
/// question.
fn connect_buds(fixture: &Fixture, bluetooth: &Arc<Bluetooth>) -> std::thread::JoinHandle<Result<(), String>> {
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    let event = fixture.until("the question", |event| !event["pairing"].is_null());
    bluetooth.answer(event["pairing"]["id"].as_u64().unwrap(), PairingAnswer::Accept).unwrap();
    connecting
}

fn connects(bluez: &FakeBlueZ) -> Vec<String> {
    bluez.calls().into_iter().filter(|call| call.contains("Connect")).collect()
}

#[test]
fn a_headset_that_connects_without_music_is_asked_for_it_again() {
    let fixture = Fixture::new("a2dp-again");
    let bluez = fixture.bluez();
    lock(&bluez.fake).refuse_a2dp = 1;
    let bluetooth = fixture.start();
    assert_eq!(connect_buds(&fixture, &bluetooth).join().unwrap(), Ok(()));
    let path = device_path(BUDS);
    assert_eq!(connects(&bluez), [format!("Connect {path}"), format!("ConnectProfile {A2DP_SINK} {path}")]);
    assert!(lock(&bluez.fake).objects.contains_key(&format!("{path}/sep1/fd0")), "music is up");
    fixture.until("the earbuds connected", |event| status(event, BUDS).as_deref() == Some("connected"));
}

#[test]
fn a_headset_that_never_plays_music_stays_connected() {
    let fixture = Fixture::new("a2dp-never");
    let bluez = fixture.bluez();
    lock(&bluez.fake).refuse_a2dp = usize::MAX;
    let bluetooth = fixture.start();
    assert_eq!(connect_buds(&fixture, &bluetooth).join().unwrap(), Ok(()));
    let path = device_path(BUDS);
    let again = format!("ConnectProfile {A2DP_SINK} {path}");
    assert_eq!(connects(&bluez), [format!("Connect {path}"), again.clone(), again], "asked twice more, then left");
    fixture.until("the earbuds connected", |event| status(event, BUDS).as_deref() == Some("connected"));
}

#[test]
fn a_device_without_music_isnt_asked_for_it() {
    let fixture = Fixture::new("a2dp-none");
    let bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    bluetooth.connect(PHONE).unwrap();
    assert_eq!(connects(&bluez), [format!("Connect {}", device_path(PHONE))]);
}

/// A connect that ends within its limit is left alone: the limit's wait
/// ends with it.
#[test]
fn a_connect_within_its_limit_is_left_alone() {
    let fixture = Fixture::new("connect-in-time");
    let bluez = fixture.bluez();
    let limit = Duration::from_millis(300);
    let bluetooth = fixture.start_with(limit);
    fixture.until("the devices", |event| event["radio"] == "on");
    bluetooth.connect(PHONE).unwrap();
    nova_test_support::settle(limit * 2);
    assert!(!bluez.calls().contains(&format!("Disconnect {}", device_path(PHONE))), "nothing stops it");
    fixture.until("the phone connected", |event| status(event, PHONE).as_deref() == Some("connected"));
}

/// A pairing question nobody answers ends at the answer timeout (an
/// answered one ends with its answer: `connect_buds`).
#[test]
fn an_unanswered_question_ends_at_the_answer_timeout() {
    let fixture = Fixture::new("unanswered");
    let _bluez = fixture.bluez();
    let bluetooth = fixture.start();
    fixture.until("the devices", |event| event["radio"] == "on");
    let connecting = {
        let bluetooth = Arc::clone(&bluetooth);
        std::thread::spawn(move || bluetooth.connect(BUDS))
    };
    fixture.until("the question", |event| !event["pairing"].is_null());
    let asked = Instant::now();
    fixture.until("the question gone", |event| event["pairing"].is_null());
    assert!(asked.elapsed() >= agent::ANSWER_TIMEOUT - Duration::from_millis(100), "{:?}", asked.elapsed());
    let _ = connecting.join().unwrap();
}
