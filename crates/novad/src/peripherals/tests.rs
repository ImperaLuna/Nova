//! The device batteries against stand-ins for the hardware only: a private
//! `dbus-daemon` per test with a fake `org.freedesktop.UPower` serving the
//! devices in `recorded/upower*.json` (and a `/sys` built from the folders
//! they name), fake hidraw nodes answering as `recorded/hidpp*.json`
//! (`fake_hidraw`), and a fake `headsetcontrol` printing
//! `recorded/headsetcontrol*.json`. Everything recorded lives in
//! `recorded/`: a new capture is a file change there.
//!
//! `headsetcontrol.json` is a real capture (`headsetcontrol -o json` with an
//! Arctis Nova 5X charging), `headsetcontrol-none.json` HeadsetControl 4.2
//! with no headset (it exits 1), `hidpp-c547.json` a Lightspeed receiver
//! and its G502 X PLUS answering HID++, `keychron-d030.json` a Keychron
//! Link dongle passing on its K2 HE's battery reports and
//! `keychron-d026.json` one with nothing behind it that sends them,
//! `keychron-0e20.json` the K2 HE on its cable answering requests,
//! `keychron-d026-mouse.json` the Link-KM's mouse interface answering
//! status requests, and `keychron-d064.json` its M6 on its cable.
//! The UPower files follow UPower's
//! D-Bus API: the desktop they're for has no peripheral in UPower (its
//! UPower lists only the DisplayDevice, which the fake serves too).

use super::*;
use crate::hub::{ClientQueue, Outgoing};
use crate::icons::Icons;
use crate::session_bus::TestBus;
use fake_hidraw::FakeHidraw;
use nova_notifications::{self as notifications, Notifications};
use nova_protocol::{LogLevel, topics};
use nova_test_support::{TempDir, wait_until, write_program};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::Receiver;
use upower::{DEVICE_INTERFACE, UPOWER, UPOWER_PATH};
use zbus::Connection;
use zbus::interface;
use zbus::wire::{OwnedObjectPath, OwnedVariant};

/// How long a test waits for what should come at once: a read, the
/// settle, and a hotplug's wait.
const WAIT: Duration = Duration::from_secs(15);
const MOUSE: &str = "/org/freedesktop/UPower/devices/mouse_hidpp_battery_0";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

fn recorded(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/peripherals/recorded").join(name)
}

/// A `recorded/upower*.json`.
#[derive(Deserialize)]
struct Recording {
    #[serde(default)]
    usb_devices: HashMap<String, String>,
    devices: Vec<RecordedDevice>,
}

#[derive(Deserialize, Clone)]
struct RecordedDevice {
    path: String,
    /// Below `/sys`, where its power supply's `device` leads.
    #[serde(default)]
    sysfs: Option<String>,
    properties: Props,
}

/// The device properties the fake serves, named as on the bus.
#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Props {
    native_path: String,
    vendor: String,
    model: String,
    serial: String,
    /// `Type`: the kind of device.
    #[serde(rename = "Type")]
    kind: u32,
    power_supply: bool,
    percentage: f64,
    battery_level: u32,
    state: u32,
    is_present: bool,
}

fn recording(name: &str) -> Recording {
    serde_json::from_str(&std::fs::read_to_string(recorded(name)).unwrap()).unwrap()
}

struct Fixture {
    dir: TempDir,
    bus: TestBus,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    client: ClientId,
    _ui_stream: UnixStream,
    hidraw: FakeHidraw,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("peripherals-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let id = hub.register(queue);
        hub.subscribe(id, topics::PERIPHERALS);
        for folder in ["sys", "bin", "usb/001"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        Self { dir, bus, hub, logger, ui, client: id, _ui_stream: ui_stream, hidraw: FakeHidraw::default() }
    }

    fn start(&self) -> Arc<Peripherals> {
        let sources = Sources {
            upower: Bus::Address(self.bus.address.clone()),
            sys_root: self.dir.join("sys"),
            dev: self.dir.join("dev"),
            open_hidraw: self.hidraw.opener(),
            path_var: self.dir.join("bin").display().to_string(),
            usb_devices: self.dir.join("usb"),
        };
        let peripherals = Peripherals::start(sources, Arc::clone(&self.hub), Arc::clone(&self.logger));
        // A new run first clears what clients heard from an earlier one.
        assert_eq!(self.next(), json!([]));
        peripherals
    }

    /// A notification server on the test's bus, low battery alerts going
    /// to it, and the UI hearing its pop-ups.
    fn notifications(&self, peripherals: &Peripherals) -> Arc<Notifications> {
        let sources = notifications::Sources {
            bus: Bus::Address(self.bus.address.clone()),
            state_file: self.dir.join("notifications.json"),
            images: self.dir.join("images"),
            icons: Arc::new(Icons::new(crate::icons::HICOLOR.into(), &[], None)),
        };
        let server = Notifications::start(
            sources,
            crate::apps::AppIndex::of(None, &self.logger),
            Arc::clone(&self.hub),
            Arc::clone(&self.logger),
        );
        self.hub.subscribe(self.client, nova_notifications_api::TOPIC);
        peripherals.set_notifications(Some(&server));
        server
    }

    /// The next notification to pop up.
    fn popup(&self) -> Value {
        crate::hub::next_named(&self.ui, "notification_popup", WAIT)
    }

    /// Serves UPower with the devices of `recorded/<name>`, their `/sys`
    /// folders made under the test's.
    fn upower(&self, names: &[&str]) -> FakeUPower {
        let sys = self.dir.join("sys");
        let mut devices = Vec::new();
        for name in names {
            let recording = recording(name);
            for (folder, ids) in &recording.usb_devices {
                let (vendor, product) = ids.split_once(':').unwrap();
                std::fs::create_dir_all(sys.join(folder)).unwrap();
                std::fs::write(sys.join(folder).join("idVendor"), format!("{vendor}\n")).unwrap();
                std::fs::write(sys.join(folder).join("idProduct"), format!("{product}\n")).unwrap();
            }
            for device in &recording.devices {
                if let Some(folder) = &device.sysfs {
                    std::fs::create_dir_all(sys.join(folder)).unwrap();
                    let supply = sys.join("class/power_supply").join(&device.properties.native_path);
                    std::fs::create_dir_all(&supply).unwrap();
                    std::os::unix::fs::symlink(sys.join(folder), supply.join("device")).unwrap();
                }
            }
            devices.extend(recording.devices);
        }
        FakeUPower::start(self.bus.connect(), devices)
    }

    /// Puts a `headsetcontrol` on the PATH that prints what
    /// `recorded/<name>` holds (as `answer` may change it) and exits
    /// `status`, like HeadsetControl, with no PATH of its own.
    fn headsetcontrol(&self, name: &str, status: i32) {
        self.headsetcontrol_answers(&std::fs::read_to_string(recorded(name)).unwrap(), status);
    }

    fn headsetcontrol_answers(&self, answer: &str, status: i32) {
        let file = self.dir.join("headsetcontrol-answer.json");
        std::fs::write(&file, answer).unwrap();
        // Shell builtins only: it runs without PATH.
        let body = format!(
            "while IFS= read -r line || [ -n \"$line\" ]; do printf '%s\\n' \"$line\"; done < '{}'\nexit {status}",
            file.display()
        );
        write_program(&self.dir.join("bin"), "headsetcontrol", &body);
    }

    /// A USB device is plugged into bus 001.
    fn plug(&self, device: &str) {
        std::fs::write(self.dir.join("usb/001").join(device), "").unwrap();
    }

    /// The devices of the next peripherals event.
    fn next(&self) -> Value {
        crate::hub::next_named(&self.ui, "peripherals", WAIT)["devices"].clone()
    }
}

fn ids(devices: &Value) -> Vec<&str> {
    devices.as_array().unwrap().iter().map(|device| device["id"].as_str().unwrap()).collect()
}

/// UPower on the test's bus, serving the devices it was given.
struct FakeUPower {
    connection: Connection,
    listed: Arc<Mutex<Vec<OwnedObjectPath>>>,
    props: HashMap<String, Arc<Mutex<Props>>>,
}

impl FakeUPower {
    fn start(connection: Connection, devices: Vec<RecordedDevice>) -> Self {
        let mut fake = Self { connection, listed: Arc::default(), props: HashMap::new() };
        let display = RecordedDevice {
            path: DISPLAY_DEVICE.into(),
            sysfs: None,
            properties: Props { kind: 2, power_supply: true, percentage: 91.0, is_present: true, ..Props::default() },
        };
        zbus::block_on(async {
            let server = fake.connection.object_server();
            server.at(UPOWER_PATH, Root { listed: Arc::clone(&fake.listed) }).await.unwrap();
            // Served, but never listed: UPower's own summary.
            server.at(DISPLAY_DEVICE, Device(Arc::new(Mutex::new(display.properties)))).await.unwrap();
        });
        for device in devices {
            fake.serve(device);
        }
        zbus::block_on(fake.connection.request_name(UPOWER)).unwrap();
        fake
    }

    fn serve(&mut self, device: RecordedDevice) {
        let props = Arc::new(Mutex::new(device.properties));
        zbus::block_on(self.connection.object_server().at(device.path.as_str(), Device(Arc::clone(&props)))).unwrap();
        lock(&self.listed).push(OwnedObjectPath::try_from(device.path.as_str()).unwrap());
        self.props.insert(device.path, props);
    }

    /// Changes device `path`, and says so as UPower does.
    fn change(&self, path: &str, change: impl FnOnce(&mut Props)) {
        change(&mut lock(&self.props[path]));
        let changed: HashMap<String, OwnedVariant> = HashMap::new();
        let signal = (DEVICE_INTERFACE, changed, Vec::<String>::new());
        let properties = "org.freedesktop.DBus.Properties";
        zbus::block_on(self.connection.emit_signal(None::<&str>, path, properties, "PropertiesChanged", &signal))
            .unwrap();
    }

    /// A device comes, as UPower says it.
    fn add(&mut self, device: RecordedDevice) {
        let path = OwnedObjectPath::try_from(device.path.as_str()).unwrap();
        self.serve(device);
        zbus::block_on(self.connection.emit_signal(None::<&str>, UPOWER_PATH, UPOWER, "DeviceAdded", &(path,)))
            .unwrap();
    }

    /// Device `path` goes, as UPower says it.
    fn remove(&mut self, path: &str) {
        lock(&self.listed).retain(|listed| listed.as_str() != path);
        self.props.remove(path);
        zbus::block_on(self.connection.object_server().remove::<Device, _>(path)).unwrap();
        let path = OwnedObjectPath::try_from(path).unwrap();
        zbus::block_on(self.connection.emit_signal(None::<&str>, UPOWER_PATH, UPOWER, "DeviceRemoved", &(path,)))
            .unwrap();
    }
}

struct Root {
    listed: Arc<Mutex<Vec<OwnedObjectPath>>>,
}

#[interface(name = "org.freedesktop.UPower")]
impl Root {
    fn enumerate_devices(&self) -> Vec<OwnedObjectPath> {
        lock(&self.listed).clone()
    }

    fn get_display_device(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(DISPLAY_DEVICE).unwrap()
    }
}

struct Device(Arc<Mutex<Props>>);

#[interface(name = "org.freedesktop.UPower.Device")]
impl Device {
    #[zbus(property)]
    fn native_path(&self) -> String {
        lock(&self.0).native_path.clone()
    }
    #[zbus(property)]
    fn vendor(&self) -> String {
        lock(&self.0).vendor.clone()
    }
    #[zbus(property)]
    fn model(&self) -> String {
        lock(&self.0).model.clone()
    }
    #[zbus(property)]
    fn serial(&self) -> String {
        lock(&self.0).serial.clone()
    }
    #[zbus(property, name = "Type")]
    fn kind(&self) -> u32 {
        lock(&self.0).kind
    }
    #[zbus(property)]
    fn power_supply(&self) -> bool {
        lock(&self.0).power_supply
    }
    #[zbus(property)]
    fn percentage(&self) -> f64 {
        lock(&self.0).percentage
    }
    #[zbus(property)]
    fn battery_level(&self) -> u32 {
        lock(&self.0).battery_level
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        lock(&self.0).state
    }
    #[zbus(property)]
    fn is_present(&self) -> bool {
        lock(&self.0).is_present
    }
}

#[test]
fn upower_lists_the_peripherals_and_follows_them() {
    let fixture = Fixture::new("upower");
    let mut upower = fixture.upower(&["upower.json"]);
    let peripherals = fixture.start();
    let devices = fixture.next();
    assert_eq!(
        devices,
        json!([
            {"id": "upower/gaming_input_dev_A0_5A_5C_11_22_33", "name": "DualSense Wireless Controller",
             "kind": "gaming_input", "link": "bluetooth", "battery": {"percent": 80}, "charging": true,
             "source": "upower"},
            {"id": "upower/keyboard_hidpp_battery_1", "name": "G915 TKL", "kind": "keyboard", "link": "dongle",
             "battery": {"level": "high"}, "charging": false, "source": "upower"},
            {"id": "upower/mouse_hidpp_battery_0", "name": "PRO X SUPERLIGHT", "kind": "mouse", "link": "dongle",
             "battery": {"percent": 55}, "charging": false, "source": "upower"},
        ]),
        "the laptop's battery, line power and the DisplayDevice are left out"
    );

    // A change that changes nothing sends nothing: the next event is the
    // charge after it.
    upower.change(MOUSE, |_| {});
    upower.change(MOUSE, |mouse| {
        mouse.percentage = 54.0;
        mouse.state = 1;
    });
    let devices = fixture.next();
    let mouse = &devices[2];
    assert_eq!((mouse["battery"].clone(), mouse["charging"].clone()), (json!({"percent": 54}), json!(true)));

    // Not present (turned off) leaves the list; present again comes back.
    upower.change(MOUSE, |mouse| mouse.is_present = false);
    assert!(!ids(&fixture.next()).contains(&"upower/mouse_hidpp_battery_0"));
    upower.change(MOUSE, |mouse| mouse.is_present = true);
    assert!(ids(&fixture.next()).contains(&"upower/mouse_hidpp_battery_0"));

    upower.remove(MOUSE);
    assert!(!ids(&fixture.next()).contains(&"upower/mouse_hidpp_battery_0"));
    let mouse = recording("upower.json").devices.into_iter().find(|device| device.path == MOUSE).unwrap();
    upower.add(mouse);
    assert!(ids(&fixture.next()).contains(&"upower/mouse_hidpp_battery_0"));
    peripherals.stop();
}

/// The desktop's own UPower: nothing but the DisplayDevice.
#[test]
fn upower_with_only_its_display_device_lists_nothing() {
    let fixture = Fixture::new("display-only");
    let _upower = fixture.upower(&[]);
    // HeadsetControl's headset comes after UPower's empty list, alone.
    fixture.headsetcontrol("headsetcontrol.json", 0);
    let peripherals = fixture.start();
    assert_eq!(ids(&fixture.next()), ["headsetcontrol/1038:2253"]);
    peripherals.stop();
}

#[test]
fn headsetcontrol_reads_the_headset_and_reads_again_on_hotplug() {
    let fixture = Fixture::new("headsetcontrol");
    fixture.headsetcontrol("headsetcontrol.json", 0);
    let peripherals = fixture.start();
    assert_eq!(
        fixture.next(),
        json!([{"id": "headsetcontrol/1038:2253", "name": "Arctis Nova 5X", "kind": "headset",
                "link": "dongle", "battery": {"percent": 30}, "charging": true,
                "source": "headsetcontrol"}])
    );

    // Turned off: the dongle answers without a battery, and the headset
    // leaves the list. A second headset it couldn't read is left out too.
    let capture = std::fs::read_to_string(recorded("headsetcontrol.json")).unwrap();
    let mut answer: Value = serde_json::from_str(&capture).unwrap();
    answer["devices"][0]["battery"] = json!({"status": "BATTERY_UNAVAILABLE", "level": -1});
    answer["devices"].as_array_mut().unwrap().push(json!({"status": "failure", "device": "HyperX Cloud Flight",
        "id_vendor": "0x0951", "id_product": "0x16c4", "capabilities": ["CAP_BATTERY_STATUS"]}));
    fixture.headsetcontrol_answers(&answer.to_string(), 0);
    fixture.plug("007");
    assert_eq!(fixture.next(), json!([]));

    // Turned on again: back.
    fixture.headsetcontrol_answers(&capture, 0);
    fixture.plug("008");
    assert_eq!(ids(&fixture.next()), ["headsetcontrol/1038:2253"]);

    // Unplugged: HeadsetControl 4.2's answer with no headset, exit 1.
    fixture.headsetcontrol("headsetcontrol-none.json", 1);
    std::fs::remove_file(fixture.dir.join("usb/001/007")).unwrap();
    assert_eq!(fixture.next(), json!([]));
    peripherals.stop();
}

/// One headset both sources report is listed once, with UPower's (the
/// kernel's) reading.
#[test]
fn a_headset_both_sources_report_is_listed_once() {
    let fixture = Fixture::new("dedupe");
    let _upower = fixture.upower(&["upower.json", "upower-kernel-headset.json"]);
    fixture.headsetcontrol("headsetcontrol.json", 0);
    let peripherals = fixture.start();
    let devices = eventually_devices(&fixture, 4);
    let headsets: Vec<&Value> =
        devices.as_array().unwrap().iter().filter(|device| device["kind"] == "headset").collect();
    assert_eq!(headsets.len(), 1, "{devices}");
    assert_eq!(headsets[0]["source"], "upower");
    assert_eq!(headsets[0]["battery"], json!({"percent": 40}));
    assert_eq!(headsets[0]["link"], "dongle", "sysfs sees a cable; HeadsetControl knows the dongle");
    peripherals.stop();
}

/// A headset on Bluetooth and on its dongle at once is listed once: no USB
/// id joins the two, its kind and name do. The dongle's entry names it, so
/// it keeps its id when the Bluetooth link drops. The merge is checked
/// alone, as the sources' lists come in any order.
#[test]
fn a_headset_on_bluetooth_and_its_dongle_is_listed_once() {
    let upower: Vec<Found> = ["upower.json", "upower-bluetooth-headset.json"]
        .into_iter()
        .flat_map(|name| recording(name).devices)
        .filter_map(|device| {
            let p = device.properties;
            let properties = upower::Properties {
                native_path: p.native_path,
                vendor: p.vendor,
                model: p.model,
                serial: p.serial,
                kind: p.kind,
                power_supply: p.power_supply,
                percentage: p.percentage,
                battery_level: p.battery_level,
                state: p.state,
                is_present: p.is_present,
            };
            upower::found(&device.path, &properties, Path::new("/nonexistent"))
        })
        .collect();
    let dongle =
        headsetcontrol::parse(std::fs::read_to_string(recorded("headsetcontrol.json")).unwrap().as_bytes()).unwrap();
    let merged =
        merge(&[(PeripheralSource::Upower, upower.clone()), (PeripheralSource::Headsetcontrol, dongle.clone())]);
    let ids: Vec<&str> = merged.devices.iter().map(|device| device.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "upower/mouse_hidpp_battery_0",
            "upower/keyboard_hidpp_battery_1",
            "upower/gaming_input_dev_A0_5A_5C_11_22_33",
            "headsetcontrol/1038:2253"
        ]
    );
    assert_eq!(merged.devices[3].link, Some(PeripheralLink::Dongle));
    let reversed = merge(&[(PeripheralSource::Headsetcontrol, dongle), (PeripheralSource::Upower, upower)]);
    let ids: Vec<&str> = reversed.devices.iter().map(|device| device.id.as_str()).collect();
    assert_eq!(ids[0], "headsetcontrol/1038:2253");
    assert!(!ids.contains(&"upower/headphones_dev_20_AF_1B_0A_FD_79"));
}

/// Only names that hold one another's words, in order, are one device's.
#[test]
fn names_match_by_whole_words() {
    assert!(names_match("SteelSeries Arctis Nova 5X", "arctis nova 5x"));
    assert!(!names_match("Arctis Nova 5", "Arctis Nova 5X"));
    assert!(!names_match("Arctis Nova 5X", "Nova 5X Arctis"));
    assert!(!names_match("", "Arctis"));
}

/// Waits for an event listing `count` devices (the sources report apart).
fn eventually_devices(fixture: &Fixture, count: usize) -> Value {
    loop {
        let devices = fixture.next();
        if devices.as_array().unwrap().len() == count {
            return devices;
        }
    }
}

#[test]
fn without_upower_or_headsetcontrol_nothing_is_listed() {
    let fixture = Fixture::new("nothing");
    let peripherals = fixture.start();
    let logs = fixture.dir.join("logs");
    wait_until(WAIT, "both sources said they're off", || {
        let log = std::fs::read_to_string(logs.join("nova.log")).unwrap_or_default();
        log.contains("UPower isn't running") && log.contains("HeadsetControl not installed")
    });
    let (client, other_end) = UnixStream::pair().unwrap();
    let (queue, sent) = ClientQueue::new(&other_end).unwrap();
    let id = fixture.hub.register(queue);
    peripherals.send_state(id);
    assert_eq!(crate::hub::next_named(&sent, "peripherals", WAIT), json!({"devices": []}));
    drop(client);
    peripherals.stop();
}

/// HeadsetControl runs with nothing from novad's environment (cargo sets
/// `DBUS_SESSION_BUS_ADDRESS` for every test).
#[test]
fn headsetcontrol_gets_no_environment() {
    let fixture = Fixture::new("environment");
    let answer = r#"{"devices":[{"status":"success","product":"%s","id_vendor":"0x1038","id_product":"0x2253","capabilities":["CAP_BATTERY_STATUS"],"battery":{"status":"BATTERY_AVAILABLE","level":50}}]}"#;
    let body = format!(
        "if [ -n \"$DBUS_SESSION_BUS_ADDRESS$HOME\" ]; then got=leaked; else got=nothing; fi\nprintf '{answer}' \"$got\""
    );
    write_program(&fixture.dir.join("bin"), "headsetcontrol", &body);
    let peripherals = fixture.start();
    assert_eq!(fixture.next()[0]["name"], "nothing");
    peripherals.stop();
}

/// The real capture: a G502 X PLUS on a Lightspeed receiver (046d:c547)
/// that the kernel left to hid-generic.
const RECEIVER: &str = "hidpp-c547.json";
const G502: &str = "hidpp/df7097d8";
/// Its ping, as novad sends it.
const G502_PING: &str = "1001001b00005a";
/// Its UNIFIED_BATTERY status request.
const G502_BATTERY: &str = "1101061b00000000000000000000000000000000";

impl Fixture {
    /// A Logitech hidraw node answering as `recorded/<name>` does, on
    /// kernel driver `driver` (the recording's own when `None`).
    fn receiver(&self, name: &str, node: &str, driver: Option<&str>) {
        let mut recording = fake_hidraw_recording(name);
        recording.node = node.into();
        if let Some(driver) = driver {
            recording.driver = driver.into();
            recording.hid = recording.hid.replace(".001B", ".0020");
        }
        self.hidraw.add(&self.dir.join("sys"), &self.dir.join("dev"), &recording);
    }
}

#[test]
fn hidpp_reads_a_mouse_on_a_receiver_the_kernel_left_alone() {
    let fixture = Fixture::new("hidpp");
    fixture.receiver(RECEIVER, "hidraw12", None);
    let peripherals = fixture.start();
    assert_eq!(
        fixture.next(),
        json!([{"id": G502, "name": "G502 X PLUS", "kind": "mouse", "link": "dongle",
                "battery": {"percent": 94}, "charging": false, "source": "hidpp"}])
    );

    let notices = fake_hidraw_recording(RECEIVER).notices;
    // Its cable plugged in: the mouse says its battery changed, and is
    // asked again.
    fixture.hidraw.answer(G502_BATTERY, &["1101061b5e080101000000000000000000000000"]);
    fixture.hidraw.say(&notices["cable_in_battery"]);
    let devices = fixture.next();
    assert_eq!((devices[0]["battery"].clone(), devices[0]["charging"].clone()), (json!({"percent": 94}), json!(true)));

    // Asleep (or on its cable): the receiver says the link is lost, and
    // answers for the slot that nothing is connected there.
    fixture.hidraw.answer(G502_PING, &["10018f001b0800"]);
    fixture.hidraw.say(&notices["cable_in_link_lost"]);
    assert_eq!(fixture.next(), json!([]));
    // Woken: the receiver says so, and it's read at once.
    let capture = recording_exchange(RECEIVER, G502_PING);
    fixture.hidraw.answer(G502_PING, &[&capture]);
    fixture.hidraw.say(&notices["woke"]);
    assert_eq!(ids(&fixture.next()), [G502]);
    peripherals.stop();
}

fn fake_hidraw_recording(name: &str) -> fake_hidraw::Recording {
    serde_json::from_str(&std::fs::read_to_string(recorded(name)).unwrap()).unwrap()
}

/// The reply a recording has for `request`.
fn recording_exchange(name: &str, request: &str) -> String {
    fake_hidraw_recording(name).exchanges.into_iter().find(|exchange| exchange.request == request).unwrap().replies[0]
        .clone()
}

/// A receiver one of the kernel's Logitech drivers has is UPower's to
/// report: its node is never opened.
#[test]
fn hidpp_leaves_receivers_the_kernel_drives_alone() {
    let fixture = Fixture::new("hidpp-kernel");
    fixture.receiver(RECEIVER, "hidraw3", Some("logitech-djreceiver"));
    fixture.receiver(RECEIVER, "hidraw12", None);
    let peripherals = fixture.start();
    assert_eq!(ids(&fixture.next()), [G502]);
    let opened = lock(&fixture.hidraw.opened).clone();
    assert_eq!(opened, [fixture.dir.join("dev/hidraw12")]);
    peripherals.stop();
}

/// A receiver plugged in later is found when its node appears, and its
/// mouse leaves the list when it's unplugged.
#[test]
fn hidpp_follows_receivers_plugged_in_and_out() {
    let fixture = Fixture::new("hidpp-hotplug");
    std::fs::create_dir_all(fixture.dir.join("dev")).unwrap();
    let peripherals = fixture.start();
    fixture.receiver(RECEIVER, "hidraw12", None);
    assert_eq!(ids(&fixture.next()), [G502]);
    // Unplugged: the node goes, and its reader's end hangs up.
    std::fs::remove_dir_all(fixture.dir.join("sys/class/hidraw/hidraw12")).unwrap();
    std::fs::remove_file(fixture.dir.join("dev/hidraw12")).unwrap();
    fixture.hidraw.unplug();
    assert_eq!(fixture.next(), json!([]));
    peripherals.stop();
}

/// The same mouse on its cable, which the kernel reads (UPower), and
/// through the receiver novad reads (HID++), is listed once, as UPower has
/// it.
#[test]
fn a_mouse_both_upower_and_hidpp_report_is_listed_once() {
    let fixture = Fixture::new("hidpp-dedupe");
    let _upower = fixture.upower(&["upower-kernel-mouse.json"]);
    fixture.receiver(RECEIVER, "hidraw12", None);
    let peripherals = fixture.start();
    // The two sources report apart: wait until both have and that's sent.
    wait_until(WAIT, "both sources reported", || {
        let state = lock(&peripherals.shared.state);
        state.pending.is_none() && state.found.iter().filter(|(_, found)| !found.is_empty()).count() == 2
    });
    let devices = serde_json::to_value(&lock(&peripherals.shared.state).sent.devices).unwrap();
    assert_eq!(ids(&devices), ["upower/mouse_hidpp_battery_2"], "{devices}");
    assert_eq!(devices[0]["source"], "upower");
    peripherals.stop();
}

/// The real capture: a K2 HE on the community battery firmware, behind
/// its Keychron Link dongle (3434:d030), and the Link-KM dongle (3434:d026)
/// with nothing behind it that sends the report.
const KEYCHRON_LINK: &str = "keychron-d030.json";
const KEYCHRON_LINK_KM: &str = "keychron-d026.json";
const KEYCHRON_CABLE: &str = "keychron-0e20.json";
/// The Link-KM's vendor node with a Keychron mouse behind it, answering.
const KEYCHRON_MOUSE: &str = "keychron-d026-mouse.json";
/// A Keychron M6 on its cable, charging; its dongle answers nothing then.
const KEYCHRON_MOUSE_CABLE: &str = "keychron-d064.json";
/// The K2 HE's answer to Keychron Launcher's protocol version request,
/// on the same interface.
const LAUNCHER_REPORT: &str = "a002000200000000000000000000000000000000000000000000000000000000";

impl Fixture {
    /// A hidraw node as `recorded/<name>` has it; its file in `/dev`.
    fn hidraw_node(&self, name: &str) -> PathBuf {
        let recording = fake_hidraw_recording(name);
        let path = self.dir.join("dev").join(&recording.node);
        self.hidraw.add(&self.dir.join("sys"), &self.dir.join("dev"), &recording);
        path
    }

    fn wait_opened(&self, node: &Path) {
        wait_until(WAIT, "the node is opened", || lock(&self.hidraw.opened).iter().any(|opened| opened == node));
    }
}

/// A keyboard behind its dongle is listed while it sends its battery, as
/// it says it, and leaves when it falls silent (turned off: the dongle
/// stays). Another dongle with the same interface lists nothing, and other
/// reports on it are ignored.
#[test]
fn keychron_lists_a_keyboard_on_its_dongle_while_it_sends() {
    let fixture = Fixture::new("keychron-dongle");
    let link = fixture.hidraw_node(KEYCHRON_LINK);
    let link_km = fixture.hidraw_node(KEYCHRON_LINK_KM);
    let peripherals = fixture.start();
    fixture.wait_opened(&link);
    fixture.wait_opened(&link_km);
    let pushes = fake_hidraw_recording(KEYCHRON_LINK).pushes;
    fixture.hidraw.say_on(&link, LAUNCHER_REPORT);
    fixture.hidraw.say_on(&link, &pushes[0]);
    assert_eq!(
        fixture.next(),
        json!([{"id": "keychron/model-3", "name": "Keychron K2 HE", "kind": "keyboard", "link": "dongle",
                "battery": {"percent": 69}, "charging": false, "source": "keychron"}])
    );
    // The percent wanders a point: nothing changes on screen.
    for push in &pushes[1..] {
        fixture.hidraw.say_on(&link, push);
    }
    // Silent past a few heartbeats: off.
    assert_eq!(fixture.next(), json!([]));
    // It was asked, as any node is, but only a few times: the dongle
    // doesn't pass requests on (a push just after one looks like an
    // answer, until the request sent to make sure goes unanswered).
    let asked = fixture.hidraw.requests(&link);
    assert!((1..=6).contains(&asked.len()), "{} requests", asked.len());
    peripherals.stop();
}

/// A report cut after its transport (no model id) still lists the
/// keyboard, by the name a Keychron keyboard on a dongle gets.
#[test]
fn keychron_reads_a_report_without_a_model() {
    let fixture = Fixture::new("keychron-short");
    let link = fixture.hidraw_node(KEYCHRON_LINK);
    let peripherals = fixture.start();
    fixture.wait_opened(&link);
    fixture.hidraw.say_on(&link, "a445220f0104");
    let devices = fixture.next();
    assert_eq!((devices[0]["name"].clone(), devices[0]["charging"].clone()), (json!("Keychron keyboard"), json!(true)));
    peripherals.stop();
}

/// On its cable the keyboard says nothing by itself and its report says
/// 2.4 GHz: it's asked, answers, is listed as wired, and asked again
/// while it stays.
#[test]
fn keychron_asks_a_keyboard_on_its_cable() {
    let fixture = Fixture::new("keychron-cable");
    let cable = fixture.hidraw_node(KEYCHRON_CABLE);
    let peripherals = fixture.start();
    assert_eq!(
        fixture.next(),
        json!([{"id": "keychron/model-3", "name": "Keychron K2 HE", "kind": "keyboard", "link": "wired",
                "battery": {"percent": 81}, "charging": true, "source": "keychron"}])
    );
    let request = fake_hidraw_recording(KEYCHRON_CABLE).exchanges[0].request.clone();
    wait_until(WAIT, "asked again", || fixture.hidraw.requests(&cable).len() >= 4);
    assert!(fixture.hidraw.requests(&cable).iter().all(|asked| *asked == fake_hidraw::hex(&request)));
    // Unplugged: its node goes.
    std::fs::remove_dir_all(fixture.dir.join("sys/class/hidraw/hidraw17")).unwrap();
    std::fs::remove_file(&cable).unwrap();
    fixture.hidraw.unplug();
    assert_eq!(fixture.next(), json!([]));
    peripherals.stop();
}

/// Plugged in by its cable, a keyboard heard behind its dongle is listed
/// once, as its cable has it, even while its dongle still passes on what
/// it says; unplugged, as its dongle has it again.
#[test]
fn keychron_lists_a_keyboard_on_cable_and_dongle_once() {
    let fixture = Fixture::new("keychron-both");
    let link = fixture.hidraw_node(KEYCHRON_LINK);
    let peripherals = fixture.start();
    fixture.wait_opened(&link);
    let dongle = fake_hidraw_recording(KEYCHRON_LINK);
    fixture.hidraw.say_on(&link, &dongle.pushes[0]);
    assert_eq!(fixture.next()[0]["link"], "dongle");
    // The cable in: the dongle says so first, then the cable's node comes.
    fixture.hidraw.say_on(&link, &dongle.notices["cable_in"]);
    assert_eq!(fixture.next()[0]["charging"], true);
    let cable = fixture.hidraw_node(KEYCHRON_CABLE);
    let devices = fixture.next();
    assert_eq!(ids(&devices), ["keychron/model-3"], "{devices}");
    assert_eq!(devices[0]["link"], "wired");
    // The dongle's heartbeat while on the cable changes nothing.
    fixture.hidraw.say_on(&link, &dongle.notices["cable_in"]);
    // Unplugged: the cable's node goes, and the dongle says it's on
    // battery.
    std::fs::remove_dir_all(fixture.dir.join("sys/class/hidraw/hidraw17")).unwrap();
    std::fs::remove_file(&cable).unwrap();
    fixture.hidraw.hang_up(&cable);
    fixture.hidraw.say_on(&link, &dongle.notices["cable_out"]);
    // The dongle's last reading may show a moment before its new one.
    let on_battery = json!({"link": "dongle", "charging": false});
    let mut devices = fixture.next();
    for _ in 0..2 {
        if json!({"link": devices[0]["link"], "charging": devices[0]["charging"]}) == on_battery {
            break;
        }
        devices = fixture.next();
    }
    assert_eq!(json!({"link": devices[0]["link"], "charging": devices[0]["charging"]}), on_battery, "{devices}");
    peripherals.stop();
}

/// A mouse behind its dongle says nothing by itself: it's asked, listed
/// while it answers, and leaves once it stops (switched off: the dongle
/// stays).
#[test]
fn keychron_asks_a_mouse_behind_its_dongle() {
    let fixture = Fixture::new("keychron-mouse");
    let dongle = fixture.hidraw_node(KEYCHRON_MOUSE);
    let peripherals = fixture.start();
    assert_eq!(
        fixture.next(),
        json!([{"id": "keychron/mouse", "name": "Keychron mouse", "kind": "mouse", "link": "dongle",
                "battery": {"percent": 94}, "charging": false, "source": "keychron"}])
    );
    let request = fake_hidraw_recording(KEYCHRON_MOUSE).exchanges[0].request.clone();
    wait_until(WAIT, "asked again", || fixture.hidraw.requests(&dongle).len() >= 2);
    assert!(fixture.hidraw.requests(&dongle).iter().all(|asked| *asked == fake_hidraw::hex(&request)));
    fixture.hidraw.answer(&request, &[]);
    assert_eq!(fixture.next(), json!([]));
    peripherals.stop();
}

/// On its cable the mouse is a device of its own: listed once, by its own
/// name, wired and charging, even while its dongle's last answer is still
/// listed.
#[test]
fn keychron_lists_a_mouse_on_its_cable_once() {
    let fixture = Fixture::new("keychron-mouse-cable");
    let dongle = fixture.hidraw_node(KEYCHRON_MOUSE);
    let peripherals = fixture.start();
    assert_eq!(fixture.next()[0]["link"], "dongle");
    fixture.hidraw_node(KEYCHRON_MOUSE_CABLE);
    assert_eq!(
        fixture.next(),
        json!([{"id": "keychron/mouse", "name": "Keychron M6", "kind": "mouse", "link": "wired",
                "battery": {"percent": 99}, "charging": true, "source": "keychron"}])
    );
    // The dongle stops answering on the cable: nothing changes.
    let request = fake_hidraw_recording(KEYCHRON_MOUSE).exchanges[0].request.clone();
    fixture.hidraw.answer(&request, &[]);
    let asked = fixture.hidraw.requests(&dongle).len();
    wait_until(WAIT, "the dongle asked twice more", || fixture.hidraw.requests(&dongle).len() >= asked + 2);
    wait_until(WAIT, "settled", || lock(&peripherals.shared.state).pending.is_none());
    let devices = serde_json::to_value(&lock(&peripherals.shared.state).sent.devices).unwrap();
    assert_eq!((ids(&devices), devices[0]["link"].clone()), (vec!["keychron/mouse"], json!("wired")));
    peripherals.stop();
}

#[test]
fn a_battery_running_down_alerts_once_at_each_step() {
    let fixture = Fixture::new("alerts");
    let upower = fixture.upower(&["upower.json"]);
    let peripherals = fixture.start();
    let server = fixture.notifications(&peripherals);
    let mut popups = std::collections::VecDeque::new();
    // Changes the mouse and waits for the list to show it, keeping the
    // pop-ups that come meanwhile: an alert comes right after its list.
    let mut read = |change: &dyn Fn(&mut Props)| {
        upower.change(MOUSE, change);
        loop {
            let event = crate::hub::next_event(&fixture.ui, WAIT);
            match event["event"].as_str() {
                Some("peripherals") => return,
                Some("notification_popup") => popups.push_back(event["data"].clone()),
                _ => {}
            }
        }
    };
    let discharging = |percent: f64| {
        move |mouse: &mut Props| {
            mouse.percentage = percent;
            mouse.state = 2;
            mouse.is_present = true;
        }
    };

    read(&discharging(20.0));
    // Under 20% but over 10%: said already.
    read(&discharging(15.0));
    read(&discharging(10.0));
    read(&discharging(4.0));
    // Away and back still low: nothing again.
    read(&|mouse| mouse.is_present = false);
    read(&discharging(4.0));
    // Charging says them all again: a fresh discharge alerts at 20% again.
    read(&|mouse| mouse.state = 1);
    read(&discharging(19.0));
    read(&discharging(18.0));
    peripherals.stop();
    server.stop();
    let mut popups = popups.into_iter().chain(std::iter::from_fn(|| Some(fixture.popup())));

    let first = popups.next().unwrap();
    assert_eq!(first["app"], alerts::APP);
    assert_eq!((first["summary"].clone(), first["body"].clone()), (json!("PRO X SUPERLIGHT"), json!("20%")));
    assert_eq!(first["urgency"], "normal");
    assert_eq!(first["device"], json!({"kind": "mouse", "battery": {"percent": 20}}));
    let low = popups.next().unwrap();
    assert_eq!(low["body"], "10%");
    assert_eq!(low["id"], first["id"], "the newer alert takes the older one's place");
    let critical = popups.next().unwrap();
    assert_eq!(critical["body"], "4%");
    assert_eq!((critical["urgency"].clone(), critical["timeout_ms"].clone()), (json!("critical"), json!(0)));
    assert_eq!(popups.next().unwrap()["body"], "19%");
}

/// What was said is gone when novad restarts, but the device's row in the
/// list isn't: the next alert takes its place instead of adding a row.
#[test]
fn an_alert_after_a_restart_takes_the_devices_row() {
    let fixture = Fixture::new("alerts-restart");
    let upower = fixture.upower(&["upower.json"]);
    upower.change(MOUSE, |mouse| {
        mouse.percentage = 9.0;
        mouse.state = 2;
    });
    let peripherals = fixture.start();
    let server = fixture.notifications(&peripherals);
    let first = fixture.popup();
    peripherals.stop();
    let peripherals = fixture.start();
    peripherals.set_notifications(Some(&server));
    let again = fixture.popup();
    peripherals.stop();
    server.stop();
    assert_eq!(first["body"], "9%");
    assert_eq!((again["id"].clone(), again["body"].clone()), (first["id"].clone(), json!("9%")));
}
