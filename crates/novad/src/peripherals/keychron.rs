//! Keychron keyboards' batteries over raw HID, from firmware that reports
//! it (the community battery patch for Keychron's QMK, first for the K10
//! HE): stock firmware sends none, and the Keychron Link dongle has no
//! battery in its HID descriptors, so nothing reaches `/sys` or UPower.
//!
//! The report is on QMK's raw HID interface (usage page `0xFF60`, usage
//! `0x61`, 32-byte reports with no report id), which the keyboard and its
//! dongle both have: `0xA4`, the percent, the voltage (2 bytes), the
//! charging state, the transport and the model. Keychron Launcher and VIA
//! use the same interface, so any other report is ignored.
//!
//! Over the dongle the keyboard sends it by itself (when the link
//! connects, when it changes, and every [`HEARTBEAT`] otherwise), and the
//! dongle doesn't pass the host's requests on; on its cable it only
//! answers a request. So each node is asked when it's opened, and again
//! every [`ASK`] while it's on a cable or until it has stayed silent for
//! [`MAX_UNANSWERED`] requests; besides that its reader only listens. A
//! dongle's node stays when its keyboard is off, so a keyboard is listed
//! only while it was heard in the last [`SILENCE`].
//!
//! The percent comes from the cell's voltage as the firmware reads it,
//! which wanders several points either way within a minute on a keyboard
//! at rest, now and then by 5 points or more in one reading (and by 17
//! when the cable goes in, a moment before the charging state says so).
//! So what's shown is the median of the readings of the last
//! [`SMOOTHING`], which one odd reading doesn't move, and it changes only
//! once that moves [`HYSTERESIS`] points from it. Starting or stopping
//! charging moves the voltage itself, so the readings start over then.
//!
//! Nodes are any of Keychron's with the raw HID usage, found as
//! [`super::hidraw`] finds them, whatever the product: one whose keyboard
//! never sends the report (stock firmware, a mouse's dongle) lists
//! nothing.

use super::hidraw::channel::{Channel, Received};
use super::hidraw::descriptor::has_usage;
use super::hidraw::{Listing, Node, Protocol};
use super::{Found, LOG_SOURCE};
use nova_protocol::{Battery, LogLevel, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Keychron's USB vendor id.
const KEYCHRON: u16 = 0x3434;
/// QMK's raw HID interface.
const RAW_HID_PAGE: u16 = 0xFF60;
const RAW_HID_USAGE: u16 = 0x61;
/// The battery report's first byte (`KC_GET_BATTERY`), and the request.
const GET_BATTERY: u8 = 0xA4;
/// A raw HID report's length; a request is written after report id 0,
/// as hidraw takes it for a device without report ids.
const REPORT_BYTES: usize = 32;
const NO_REPORT_ID: u8 = 0;
/// The bytes a battery report needs: up to its transport. A model id
/// after it is optional.
const MIN_REPORT_BYTES: usize = 6;
/// The charging states: on battery, charging, full (on power, not
/// charging).
const CHARGING: u8 = 1;
/// The transports: USB (its cable), Bluetooth, 2.4 GHz (the dongle).
const TRANSPORT_USB: u8 = 0x01;
const TRANSPORT_BLUETOOTH: u8 = 0x02;
const TRANSPORT_DONGLE: u8 = 0x04;
/// The models that say who they are.
const MODELS: [(u8, &str); 3] = [(1, "Keychron K10 HE"), (2, "Keychron Q1 HE"), (3, "Keychron K2 HE")];
/// What an unknown model on the dongle is called: the dongle's own name
/// (`Keychron Link`) isn't the keyboard's.
const UNKNOWN_KEYBOARD: &str = "Keychron keyboard";
/// The highest percent there is.
const FULL: u8 = 100;
/// How often a keyboard on the dongle sends the report by itself when
/// nothing changed...
pub const HEARTBEAT: Duration = Duration::from_secs(HEARTBEAT_SECS);
const HEARTBEAT_SECS: u64 = 30;
/// ...so one not heard this long (two and a half heartbeats) is off.
pub const SILENCE: Duration =
    if cfg!(test) { Duration::from_millis(1500) } else { Duration::from_millis(HEARTBEAT_SECS * 2500) };
/// How often a node is asked: one on a cable says nothing otherwise.
pub const ASK: Duration = if cfg!(test) { Duration::from_millis(500) } else { HEARTBEAT };
/// Requests in a row a node may leave unanswered before it's only
/// listened to: a dongle doesn't pass them on.
const MAX_UNANSWERED: u32 = 3;
/// How far back the readings that make the shown percent go...
const SMOOTHING: Duration = Duration::from_secs(180);
/// ...and how far their median moves from it before it changes.
const HYSTERESIS: u8 = 2;
/// The longest a reader waits with nothing due: it only listens then.
const IDLE_WAIT: Duration = Duration::from_secs(3600);
/// The id a keyboard's comes after.
const ID_PREFIX: &str = "keychron/";

/// Keychron's raw HID battery report.
pub struct Keychron;

impl Protocol for Keychron {
    const VENDOR: u16 = KEYCHRON;
    const NAME: &'static str = "Keychron";

    fn reads(&self, node: &Node) -> bool {
        has_usage(&node.descriptor, RAW_HID_PAGE, RAW_HID_USAGE)
    }

    fn read(&self, node: &Node, mut channel: Channel, listing: &Listing) -> Option<String> {
        // What's shown, and when the keyboard was last heard.
        let mut heard: Option<(Smoothed, Instant)> = None;
        // Whether it last said it's on its cable, where it only answers.
        let mut on_cable = false;
        let mut can_ask = true;
        let mut unanswered = 0;
        let mut next_ask = Some(Instant::now());
        loop {
            let now = Instant::now();
            if next_ask.is_some_and(|at| at <= now) {
                match channel.send(&request()) {
                    Ok(true) => {}
                    Ok(false) => return None,
                    // It can still be listened to.
                    Err(e) => {
                        let problem = format!("{}: can't ask for the battery ({e}); only listening", node.name);
                        listing.logger().log(LogLevel::Debug, LOG_SOURCE, &problem);
                        can_ask = false;
                    }
                }
                unanswered += 1;
                next_ask = (can_ask && (on_cable || unanswered < MAX_UNANSWERED)).then_some(now + ASK);
            }
            if heard.as_ref().is_some_and(|(_, at)| *at + SILENCE <= now) {
                heard = None;
                listing.set(Vec::new());
            }
            let expires = heard.as_ref().map(|(_, at)| *at + SILENCE);
            let until = [next_ask, expires].into_iter().flatten().min().unwrap_or(now + IDLE_WAIT);
            match channel.receive(until) {
                Ok(Received::Report(report)) => {
                    let Some(reading) = Reading::parse(&report) else { continue };
                    let now = Instant::now();
                    unanswered = 0;
                    let smoothed = match heard.take() {
                        Some((mut smoothed, _)) => {
                            smoothed.add(reading, now);
                            smoothed
                        }
                        None => Smoothed::new(reading, now),
                    };
                    // Unchanged lists aren't sent on.
                    listing.set(vec![found(node, smoothed.shown())]);
                    heard = Some((smoothed, now));
                    // Off its cable it says it by itself.
                    on_cable = reading.on_cable();
                    next_ask = (on_cable && can_ask).then_some(now + ASK);
                }
                Ok(Received::TimedOut) => {}
                Ok(Received::Stopped) => return None,
                Err(e) => return Some(e.to_string()),
            }
        }
    }
}

/// A keyboard's readings, and the percent shown for them.
struct Smoothed {
    last: Reading,
    /// The percents of the last [`SMOOTHING`], and when they came.
    recent: VecDeque<(Instant, u8)>,
    shown: u8,
}

impl Smoothed {
    fn new(reading: Reading, at: Instant) -> Self {
        Self { last: reading, recent: VecDeque::from([(at, reading.percent)]), shown: reading.percent }
    }

    fn add(&mut self, reading: Reading, at: Instant) {
        // Plugged in or out: the voltage jumps, and older readings say
        // nothing about the charge now.
        if (reading.charging, reading.transport) != (self.last.charging, self.last.transport) {
            *self = Self::new(reading, at);
            return;
        }
        self.last = reading;
        self.recent.push_back((at, reading.percent));
        while self.recent.front().is_some_and(|(then, _)| at.duration_since(*then) > SMOOTHING) {
            self.recent.pop_front();
        }
        let mut percents: Vec<u8> = self.recent.iter().map(|(_, percent)| *percent).collect();
        percents.sort_unstable();
        // The middle one, or the lower of the middle two: one odd reading
        // either way doesn't move it.
        let median = percents[(percents.len() - 1) / 2];
        if median.abs_diff(self.shown) >= HYSTERESIS {
            self.shown = median;
        }
    }

    /// The last reading, with the percent shown.
    fn shown(&self) -> Reading {
        Reading { percent: self.shown, ..self.last }
    }
}

/// The battery request.
fn request() -> Vec<u8> {
    let mut request = vec![0; 1 + REPORT_BYTES];
    request[0] = NO_REPORT_ID;
    request[1] = GET_BATTERY;
    request
}

/// One battery report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reading {
    percent: u8,
    charging: bool,
    transport: u8,
    model: u8,
}

impl Reading {
    /// `report`, if it's a battery report.
    fn parse(report: &[u8]) -> Option<Self> {
        if report.len() < MIN_REPORT_BYTES || report[0] != GET_BATTERY || report[1] > FULL {
            return None;
        }
        Some(Self {
            percent: report[1],
            charging: report[4] == CHARGING,
            transport: report[5],
            model: report.get(6).copied().unwrap_or(0),
        })
    }

    fn on_cable(self) -> bool {
        self.transport == TRANSPORT_USB
    }
}

/// The keyboard `reading` came from, through `node`.
fn found(node: &Node, reading: Reading) -> Found {
    let link = match reading.transport {
        TRANSPORT_USB => Some(PeripheralLink::Wired),
        TRANSPORT_BLUETOOTH => Some(PeripheralLink::Bluetooth),
        TRANSPORT_DONGLE => Some(PeripheralLink::Dongle),
        _ => None,
    };
    let model = MODELS.iter().find(|(id, _)| *id == reading.model).map(|(_, name)| (*name).to_owned());
    // On its cable the node is the keyboard's own, and so is its name.
    let own_name = node.hid_name.clone().filter(|_| reading.on_cable());
    let device = Peripheral {
        id: format!("{ID_PREFIX}{}", node.id),
        name: model.or(own_name).unwrap_or_else(|| UNKNOWN_KEYBOARD.to_owned()),
        kind: PeripheralKind::Keyboard,
        link,
        battery: Some(Battery::Percent(reading.percent)),
        charging: reading.charging,
        source: PeripheralSource::Keychron,
    };
    Found { device, usb_ids: vec![node.id], serial: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peripherals::fake_hidraw::hex;

    #[test]
    fn battery_reports_are_read_and_others_ignored() {
        let reading = Reading::parse(&hex("a44e1c100104030000")).unwrap();
        assert_eq!(reading, Reading { percent: 78, charging: true, transport: TRANSPORT_DONGLE, model: 3 });
        // Six bytes: no model id.
        assert_eq!(Reading::parse(&hex("a4320f0f0001")).unwrap().model, 0);
        assert_eq!(Reading::parse(&hex("a4320f0f0201")).map(|reading| reading.charging), Some(false), "full");
        // Launcher's traffic, a cut-off report, a percent past 100.
        assert_eq!(Reading::parse(&hex("a10102030405060708")), None);
        assert_eq!(Reading::parse(&hex("a4320f0f00")), None);
        assert_eq!(Reading::parse(&hex("a4ff0f0f000403")), None);
    }

    /// The percents a K2 HE at rest sent over the dongle on RyzenShine in
    /// two captures a minute long (`recorded/keychron-d030.json` has the
    /// second).
    const AT_REST: [u8; 12] = [74, 72, 71, 66, 72, 71, 71, 69, 70, 69, 73, 69];

    fn on_battery(percent: u8) -> Reading {
        Reading { percent, charging: false, transport: TRANSPORT_DONGLE, model: 3 }
    }

    /// The percents shown as `percents` come, 10 s apart.
    fn shown(smoothed: &mut Smoothed, start: Instant, percents: &[u8]) -> Vec<u8> {
        let mut shown = Vec::new();
        for (at, percent) in (1u64..).zip(percents) {
            smoothed.add(on_battery(*percent), start + Duration::from_secs(at * 10));
            shown.push(smoothed.shown().percent);
        }
        shown.dedup();
        shown
    }

    #[test]
    fn a_wandering_percent_is_shown_steady() {
        let start = Instant::now();
        let mut smoothed = Smoothed::new(on_battery(AT_REST[0]), start);
        assert_eq!(shown(&mut smoothed, start, &AT_REST[1..]), [72], "it follows the median, not each reading");
        // One dip, or the jump of the cable going in before the charging
        // state says so, doesn't show.
        assert_eq!(shown(&mut smoothed, start + Duration::from_secs(120), &[66, 71, 88, 70]), [72]);
    }

    #[test]
    fn a_real_drop_shows_and_charging_starts_over() {
        let start = Instant::now();
        let mut smoothed = Smoothed::new(on_battery(70), start);
        // Old readings leave the window: a lasting drop shows in full.
        for minute in 1..=4 {
            smoothed.add(on_battery(60), start + Duration::from_secs(minute * 60));
        }
        assert_eq!(smoothed.shown().percent, 60);
        // On power the voltage jumps: it's shown as it comes.
        let charging = Reading { charging: true, ..on_battery(78) };
        smoothed.add(charging, start + Duration::from_secs(300));
        assert_eq!(smoothed.shown(), charging);
    }

    #[test]
    fn the_request_is_a_whole_report_after_report_id_0() {
        let request = request();
        assert_eq!((request.len(), request[0], request[1]), (33, 0, GET_BATTERY));
        assert!(request[2..].iter().all(|byte| *byte == 0));
    }
}
