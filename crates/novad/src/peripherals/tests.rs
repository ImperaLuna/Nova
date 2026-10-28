//! The device batteries against stand-ins for the hardware only: a private
//! `dbus-daemon` per test with a fake `org.freedesktop.UPower` serving the
//! devices in `recorded/upower*.json` (and a `/sys` built from the folders
//! they name), and a fake `headsetcontrol` printing `recorded/headsetcontrol*.json`.
//! Everything recorded lives in `recorded/`: a new capture is a file change
//! there.
//!
//! `headsetcontrol.json` is a real capture (`headsetcontrol -o json` with an
//! Arctis Nova 5X charging), `headsetcontrol-none.json` HeadsetControl 4.2
//! with no headset (it exits 1). The UPower files follow UPower's D-Bus
//! API: the desktop they're for has no peripheral in UPower yet.

use super::*;
use crate::hub::{ClientQueue, Outgoing};
use crate::session_bus::TestBus;
use nova_protocol::{LogLevel, Topic};
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
    _ui_stream: UnixStream,
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
        hub.subscribe(id, Topic::Peripherals);
        for folder in ["sys", "bin", "usb/001"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        Self { dir, bus, hub, logger, ui, _ui_stream: ui_stream }
    }

    fn start(&self) -> Arc<Peripherals> {
        let sources = Sources {
            upower: Bus::Address(self.bus.address.clone()),
            sys_root: self.dir.join("sys"),
            path_var: self.dir.join("bin").display().to_string(),
            usb_devices: self.dir.join("usb"),
        };
        Peripherals::start(sources, Arc::clone(&self.hub), Arc::clone(&self.logger))
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
