use super::*;

const RECEIVER: &str = "hidpp-c547.json";
const G502: &str = "hidpp/df7097d8";
const G502_PING: &str = "1001001b00005a";
const G502_BATTERY: &str = "1101061b00000000000000000000000000000000";

impl Fixture {
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
    fixture.hidraw.answer(G502_BATTERY, &["1101061b5e080101000000000000000000000000"]);
    fixture.hidraw.say(&notices["cable_in_battery"]);
    let devices = fixture.next();
    assert_eq!((devices[0]["battery"].clone(), devices[0]["charging"].clone()), (json!({"percent": 94}), json!(true)));

    fixture.hidraw.answer(G502_PING, &["10018f001b0800"]);
    fixture.hidraw.say(&notices["cable_in_link_lost"]);
    assert_eq!(fixture.next(), json!([]));
    let capture = recording_exchange(RECEIVER, G502_PING);
    fixture.hidraw.answer(G502_PING, &[&capture]);
    fixture.hidraw.say(&notices["woke"]);
    assert_eq!(ids(&fixture.next()), [G502]);
    peripherals.stop();
}

fn recording_exchange(name: &str, request: &str) -> String {
    fake_hidraw_recording(name).exchanges.into_iter().find(|exchange| exchange.request == request).unwrap().replies[0]
        .clone()
}

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

#[test]
fn hidpp_follows_receivers_plugged_in_and_out() {
    let fixture = Fixture::new("hidpp-hotplug");
    std::fs::create_dir_all(fixture.dir.join("dev")).unwrap();
    let peripherals = fixture.start();
    fixture.receiver(RECEIVER, "hidraw12", None);
    assert_eq!(ids(&fixture.next()), [G502]);
    std::fs::remove_dir_all(fixture.dir.join("sys/class/hidraw/hidraw12")).unwrap();
    std::fs::remove_file(fixture.dir.join("dev/hidraw12")).unwrap();
    fixture.hidraw.unplug();
    assert_eq!(fixture.next(), json!([]));
    peripherals.stop();
}

#[test]
fn a_mouse_both_upower_and_hidpp_report_is_listed_once() {
    let fixture = Fixture::new("hidpp-dedupe");
    let _upower = fixture.upower(&["upower-kernel-mouse.json"]);
    fixture.receiver(RECEIVER, "hidraw12", None);
    let peripherals = fixture.start();
    wait_until(WAIT, "both sources reported", || {
        let state = lock(&peripherals.shared.state);
        state.pending.is_none() && state.found.iter().filter(|(_, found)| !found.is_empty()).count() == 2
    });
    let devices = serde_json::to_value(&lock(&peripherals.shared.state).sent.devices).unwrap();
    assert_eq!(ids(&devices), ["upower/mouse_hidpp_battery_2"], "{devices}");
    assert_eq!(devices[0]["source"], "upower");
    peripherals.stop();
}
