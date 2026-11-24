use super::Found;
use super::LOG_SOURCE;
use super::hidraw::channel::{Channel, Received};
use super::hidraw::descriptor::has_usage;
use super::hidraw::{Listing, Node};
use nova_carrier::log::LogLevel;
use nova_peripherals_api::{Battery, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource};
use std::time::{Duration, Instant};

const VENDOR_PAGE: u16 = 0xFFC1;
const VENDOR_USAGE: u16 = 0x01;
const REQUEST: u8 = 0xB3;
const ANSWER: u8 = 0xB4;
const STATUS: u8 = 0x06;
const REQUEST_BYTES: usize = 63;
const BATTERY_AT: usize = 20;
const CHARGING: u8 = 0x80;
const FULL: u8 = 100;
const ASK: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(60) };
const RETRY: Duration = if cfg!(test) { Duration::from_millis(100) } else { Duration::from_secs(5) };
const ANSWER_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_MISSED: u32 = 2;
const NAME: &str = "Keychron mouse";
const DONGLE_WORD: &str = "Link";
const ID: &str = "keychron/mouse";

pub fn reads(node: &Node) -> bool {
    has_usage(&node.descriptor, VENDOR_PAGE, VENDOR_USAGE)
}

pub fn read(node: &Node, mut channel: Channel, listing: &Listing) -> Option<String> {
    let mut listed = false;
    let mut missed = 0;
    loop {
        match channel.send(&request()) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(e) => return Some(format!("can't ask for the battery ({e})")),
        }
        let asked = Instant::now();
        let reading = match answer(&mut channel, asked + ANSWER_TIMEOUT) {
            Ok(reading) => reading,
            Err(ended) => return ended,
        };
        if let Some(reading) = reading {
            missed = 0;
            listed = true;
            listing.set(vec![found(node, reading)]);
        } else {
            missed += 1;
            if listed && missed >= MAX_MISSED {
                listed = false;
                let quiet = format!("{}: the mouse stopped answering", node.name);
                listing.logger().log(LogLevel::Debug, LOG_SOURCE, &quiet);
                listing.set(Vec::new());
            }
        }
        let next_ask = asked + if reading.is_some() { ASK } else { RETRY };
        loop {
            match channel.receive(next_ask) {
                Ok(Received::Report(_)) => {}
                Ok(Received::TimedOut) => break,
                Ok(Received::Stopped) => return None,
                Err(e) => return Some(e.to_string()),
            }
        }
    }
}

fn answer(channel: &mut Channel, until: Instant) -> Result<Option<Reading>, Option<String>> {
    loop {
        match channel.receive(until) {
            Ok(Received::Report(report)) => {
                if let Some(reading) = parse(&report) {
                    return Ok(Some(reading));
                }
            }
            Ok(Received::TimedOut) => return Ok(None),
            Ok(Received::Stopped) => return Err(None),
            Err(e) => return Err(Some(e.to_string())),
        }
    }
}

fn request() -> Vec<u8> {
    let mut request = vec![0; 1 + REQUEST_BYTES];
    request[0] = REQUEST;
    request[1] = STATUS;
    request
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reading {
    percent: u8,
    charging: bool,
}

fn parse(report: &[u8]) -> Option<Reading> {
    let battery = *report.get(BATTERY_AT)?;
    let reading = Reading { percent: battery & !CHARGING, charging: battery & CHARGING != 0 };
    (report[0] == ANSWER && report[1] == STATUS && reading.percent <= FULL).then_some(reading)
}

fn found(node: &Node, reading: Reading) -> Found {
    let own_name = node.hid_name.as_deref().filter(|name| !name.contains(DONGLE_WORD)).map(without_maker_twice);
    let link = if own_name.is_some() { PeripheralLink::Wired } else { PeripheralLink::Dongle };
    let device = Peripheral {
        id: ID.to_owned(),
        name: own_name.unwrap_or_else(|| NAME.to_owned()),
        kind: PeripheralKind::Mouse,
        link: Some(link),
        battery: Some(Battery::Percent(reading.percent)),
        charging: reading.charging,
        source: PeripheralSource::Keychron,
    };
    Found { device, usb_ids: vec![node.id], serial: None }
}

fn without_maker_twice(name: &str) -> String {
    let mut words = name.split_whitespace();
    match (words.next(), words.clone().next()) {
        (Some(maker), Some(again)) if maker == again => words.collect::<Vec<_>>().join(" "),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_hidraw::hex;

    #[test]
    fn the_status_answer_is_read_and_others_ignored() {
        let answer = "b40600222202900120034006800c88131505041e5e0000000000003700000004";
        assert_eq!(parse(&hex(answer)), Some(Reading { percent: 94, charging: false }));
        let charging = answer.replacen("1e5e", "1ee2", 1);
        assert_eq!(parse(&hex(&charging)), Some(Reading { percent: 98, charging: true }));
        assert_eq!(parse(&hex(&answer.replacen("b406", "b407", 1))), None);
        assert_eq!(parse(&hex(&answer[..40])), None);
        assert_eq!(parse(&hex(&answer.replacen("1e5e", "1eff", 1))), None);
    }

    #[test]
    fn the_maker_is_named_once() {
        assert_eq!(without_maker_twice("Keychron Keychron M6"), "Keychron M6");
        assert_eq!(without_maker_twice("Keychron M6"), "Keychron M6");
    }

    #[test]
    fn the_request_is_a_whole_report() {
        let request = request();
        assert_eq!((request.len(), request[0], request[1]), (64, REQUEST, STATUS));
        assert!(request[2..].iter().all(|byte| *byte == 0));
    }
}
