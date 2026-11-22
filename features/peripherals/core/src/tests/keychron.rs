//! Keychron keyboards and mice, on their dongles and cables, against
//! `recorded/keychron*.json`.

use super::*;

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
