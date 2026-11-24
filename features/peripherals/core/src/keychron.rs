use super::hidraw::channel::{Channel, Received};
use super::hidraw::descriptor::has_usage;
use super::hidraw::{Listing, Node, Protocol};
use super::{Found, LOG_SOURCE, keychron_mouse};
use nova_carrier::log::LogLevel;
use nova_peripherals_api::{Battery, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const KEYCHRON: u16 = 0x3434;
const RAW_HID_PAGE: u16 = 0xFF60;
const RAW_HID_USAGE: u16 = 0x61;
const GET_BATTERY: u8 = 0xA4;
const REPORT_BYTES: usize = 32;
const NO_REPORT_ID: u8 = 0;
const MIN_REPORT_BYTES: usize = 6;
const CHARGING: u8 = 1;
const TRANSPORT_USB: u8 = 0x01;
const TRANSPORT_BLUETOOTH: u8 = 0x02;
const TRANSPORT_DONGLE: u8 = 0x04;
const MODELS: [(u8, &str); 3] = [(1, "Keychron K10 HE"), (2, "Keychron Q1 HE"), (3, "Keychron K2 HE")];
const UNKNOWN_KEYBOARD: &str = "Keychron keyboard";
const FULL: u8 = 100;
pub const HEARTBEAT: Duration = Duration::from_secs(HEARTBEAT_SECS);
const HEARTBEAT_SECS: u64 = 30;
pub const SILENCE: Duration =
    if cfg!(test) { Duration::from_millis(1500) } else { Duration::from_millis(HEARTBEAT_SECS * 2500) };
pub const ASK: Duration = if cfg!(test) { Duration::from_millis(500) } else { HEARTBEAT };
const MAX_UNANSWERED: u32 = 3;
const ANSWER_TIMEOUT: Duration = Duration::from_millis(100);
const CONFIRMATIONS: u32 = 2;
const SMOOTHING: Duration = Duration::from_secs(180);
const HYSTERESIS: u8 = 2;
const OUTLIER: u8 = 5;
const IDLE_WAIT: Duration = Duration::from_secs(3600);
const ID_PREFIX: &str = "keychron/";

pub struct Keychron;

impl Protocol for Keychron {
    const VENDOR: u16 = KEYCHRON;
    const NAME: &'static str = "Keychron";

    fn reads(&self, node: &Node) -> bool {
        has_usage(&node.descriptor, RAW_HID_PAGE, RAW_HID_USAGE) || keychron_mouse::reads(node)
    }

    fn read(&self, node: &Node, mut channel: Channel, listing: &Listing) -> Option<String> {
        if keychron_mouse::reads(node) {
            return keychron_mouse::read(node, channel, listing);
        }
        let mut heard: Option<(Smoothed, Instant)> = None;
        let mut asking = Asking::new(Instant::now());
        loop {
            let now = Instant::now();
            if asking.asked.is_some_and(|at| at + ANSWER_TIMEOUT <= now) {
                asking.asked = None;
                asking.answered = 0;
            }
            if asking.next_ask.is_some_and(|at| at <= now) {
                match channel.send(&request()) {
                    Ok(true) => asking.asked = Some(now),
                    Ok(false) => return None,
                    Err(e) => {
                        let problem = format!("{}: can't ask for the battery ({e}); only listening", node.name);
                        listing.logger().log(LogLevel::Debug, LOG_SOURCE, &problem);
                        asking.can_ask = false;
                    }
                }
                asking.unanswered += 1;
                asking.next_ask =
                    (asking.can_ask && (asking.wired || asking.unanswered < MAX_UNANSWERED)).then_some(now + ASK);
            }
            if heard.as_ref().is_some_and(|(_, at)| *at + SILENCE <= now) {
                heard = None;
                listing.set(Vec::new());
            }
            let expires = heard.as_ref().map(|(_, at)| *at + SILENCE);
            let answer_due = asking.asked.map(|at| at + ANSWER_TIMEOUT);
            let until = [asking.next_ask, expires, answer_due].into_iter().flatten().min().unwrap_or(now + IDLE_WAIT);
            match channel.receive(until) {
                Ok(Received::Report(report)) => {
                    let Some(reading) = Reading::parse(&report) else { continue };
                    let now = Instant::now();
                    asking.heard(now);
                    let smoothed = match heard.take() {
                        Some((mut smoothed, _)) => {
                            smoothed.add(reading, now);
                            smoothed
                        }
                        None => Smoothed::new(reading, now),
                    };
                    listing.set(vec![found(node, smoothed.shown(), asking.wired)]);
                    heard = Some((smoothed, now));
                    if asking.wired && asking.can_ask {
                        asking.next_ask = Some(now + ASK);
                    }
                }
                Ok(Received::TimedOut) => {}
                Ok(Received::Stopped) => return None,
                Err(e) => return Some(e.to_string()),
            }
        }
    }
}

struct Asking {
    wired: bool,
    can_ask: bool,
    unanswered: u32,
    answered: u32,
    asked: Option<Instant>,
    next_ask: Option<Instant>,
}

impl Asking {
    fn new(now: Instant) -> Self {
        Self { wired: false, can_ask: true, unanswered: 0, answered: 0, asked: None, next_ask: Some(now) }
    }

    fn heard(&mut self, now: Instant) {
        if !self.asked.take().is_some_and(|at| now.duration_since(at) <= ANSWER_TIMEOUT) {
            self.answered = 0;
            return;
        }
        self.unanswered = 0;
        self.answered += 1;
        if self.answered >= CONFIRMATIONS {
            self.wired = true;
        } else if self.can_ask {
            self.next_ask = Some(now);
        }
    }
}

struct Smoothed {
    last: Reading,
    recent: VecDeque<(Instant, u8)>,
    shown: u8,
    last_odd: bool,
}

impl Smoothed {
    fn new(reading: Reading, at: Instant) -> Self {
        Self { last: reading, recent: VecDeque::from([(at, reading.percent)]), shown: reading.percent, last_odd: false }
    }

    fn add(&mut self, reading: Reading, at: Instant) {
        if (reading.charging, reading.transport) != (self.last.charging, self.last.transport) {
            *self = Self::new(reading, at);
            return;
        }
        self.last = reading;
        let odd = reading.percent.abs_diff(self.shown) >= OUTLIER;
        let was_odd = std::mem::replace(&mut self.last_odd, odd);
        if odd && !was_odd {
            return;
        }
        self.recent.push_back((at, reading.percent));
        while self.recent.front().is_some_and(|(then, _)| at.duration_since(*then) > SMOOTHING) {
            self.recent.pop_front();
        }
        let mut percents: Vec<u8> = self.recent.iter().map(|(_, percent)| *percent).collect();
        percents.sort_unstable();
        let median = percents[(percents.len() - 1) / 2];
        if median.abs_diff(self.shown) >= HYSTERESIS {
            self.shown = median;
        }
    }

    fn shown(&self) -> Reading {
        Reading { percent: self.shown, ..self.last }
    }
}

fn request() -> Vec<u8> {
    let mut request = vec![0; 1 + REPORT_BYTES];
    request[0] = NO_REPORT_ID;
    request[1] = GET_BATTERY;
    request
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reading {
    percent: u8,
    charging: bool,
    transport: u8,
    model: u8,
}

impl Reading {
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
}

fn found(node: &Node, reading: Reading, wired: bool) -> Found {
    let link = match reading.transport {
        _ if wired => Some(PeripheralLink::Wired),
        TRANSPORT_USB => Some(PeripheralLink::Wired),
        TRANSPORT_BLUETOOTH => Some(PeripheralLink::Bluetooth),
        TRANSPORT_DONGLE => Some(PeripheralLink::Dongle),
        _ => None,
    };
    let model = MODELS.iter().find(|(id, _)| *id == reading.model).map(|(_, name)| (*name).to_owned());
    let own_name = node.hid_name.clone().filter(|_| wired);
    let id = match reading.model {
        0 => format!("{ID_PREFIX}{}", node.id),
        known => format!("{ID_PREFIX}model-{known}"),
    };
    let device = Peripheral {
        id,
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
    use crate::fake_hidraw::hex;

    #[test]
    fn battery_reports_are_read_and_others_ignored() {
        let reading = Reading::parse(&hex("a44e1c100104030000")).unwrap();
        assert_eq!(reading, Reading { percent: 78, charging: true, transport: TRANSPORT_DONGLE, model: 3 });
        assert_eq!(Reading::parse(&hex("a4320f0f0001")).unwrap().model, 0);
        assert_eq!(Reading::parse(&hex("a4320f0f0201")).map(|reading| reading.charging), Some(false), "full");
        assert_eq!(Reading::parse(&hex("a10102030405060708")), None);
        assert_eq!(Reading::parse(&hex("a4320f0f00")), None);
        assert_eq!(Reading::parse(&hex("a4ff0f0f000403")), None);
    }

    const AT_REST: [u8; 12] = [74, 72, 71, 66, 72, 71, 71, 69, 70, 69, 73, 69];

    fn on_battery(percent: u8) -> Reading {
        Reading { percent, charging: false, transport: TRANSPORT_DONGLE, model: 3 }
    }

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
        assert_eq!(shown(&mut smoothed, start + Duration::from_secs(120), &[66, 71, 88, 70]), [72]);
    }

    #[test]
    fn single_deep_dips_never_show() {
        let start = Instant::now();
        let mut smoothed = Smoothed::new(on_battery(69), start);
        assert_eq!(shown(&mut smoothed, start, &[58, 69, 55, 68, 68, 68, 68, 73, 68, 70, 68]), [69]);
    }

    #[test]
    fn a_real_drop_shows_and_charging_starts_over() {
        let start = Instant::now();
        let mut smoothed = Smoothed::new(on_battery(70), start);
        for minute in 1..=4 {
            smoothed.add(on_battery(60), start + Duration::from_secs(minute * 60));
        }
        assert_eq!(smoothed.shown().percent, 60);
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
