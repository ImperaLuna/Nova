//! Keychron mice's batteries, over the vendor interface their dongle
//! (Keychron Link-KM, Ultra-Link) and their cable have: usage page
//! `0xFFC1`, 63-byte reports with report ids. Stock firmware answers
//! there, unlike the keyboards' raw HID report ([`super::keychron`]), and
//! nothing reaches `/sys` or UPower without it.
//!
//! The status request is report `0xB3` with command `0x06`; the answer is
//! report `0xB4`, the command again, the mouse's settings (its DPI
//! stages) and, at byte [`BATTERY_AT`], its percent, with the top bit set
//! while it charges (`0xE3` is 99% on its cable). The mouse says
//! nothing by itself, so it's asked when the node is opened, then every
//! [`ASK`] while it answers (within [`ANSWER_TIMEOUT`]) and every
//! [`RETRY`] while it doesn't, so one switched on shows within seconds.
//! It leaves the list after [`MAX_MISSED`] unanswered requests in a row
//! (off, or no mouse behind the dongle), so one lost answer doesn't hide
//! it.
//!
//! On its cable the mouse is a USB device of its own, named for its model
//! (`Keychron M6`), and its dongle stops answering; Keychron names its
//! dongles `Link` something, so a node named so is a dongle. Both list it
//! under one id, so while the dongle's last answer is still listed the
//! cable's wins ([`super::hidraw`]): two Keychron mice at once would show
//! as one. The same protocol is what `keychron-battery-dkms` speaks to
//! the M5 and the Ultra-Link 8K.

use super::Found;
use super::LOG_SOURCE;
use super::hidraw::channel::{Channel, Received};
use super::hidraw::descriptor::has_usage;
use super::hidraw::{Listing, Node};
use nova_carrier::log::LogLevel;
use nova_peripherals_api::{Battery, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource};
use std::time::{Duration, Instant};

/// The vendor interface's collection.
const VENDOR_PAGE: u16 = 0xFFC1;
const VENDOR_USAGE: u16 = 0x01;
/// The request's report id and the answer's, and the status command both
/// carry.
const REQUEST: u8 = 0xB3;
const ANSWER: u8 = 0xB4;
const STATUS: u8 = 0x06;
/// A request's length after its report id.
const REQUEST_BYTES: usize = 63;
/// Where the battery byte is in the answer, report id included, and its
/// charging bit; the rest is the percent.
const BATTERY_AT: usize = 20;
const CHARGING: u8 = 0x80;
/// The highest percent there is.
const FULL: u8 = 100;
/// How often the mouse is asked while it answers: its percent moves a
/// point every few hours of use...
const ASK: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(60) };
/// ...and while it doesn't, so one switched on shows soon.
const RETRY: Duration = if cfg!(test) { Duration::from_millis(100) } else { Duration::from_secs(5) };
/// How soon an answer comes (a few milliseconds through the dongle).
const ANSWER_TIMEOUT: Duration = Duration::from_millis(500);
/// Unanswered requests in a row before the mouse leaves the list.
const MAX_MISSED: u32 = 2;
/// What a mouse behind its dongle is called: neither the dongle's name
/// (`Keychron Link-KM`) nor the answer says which model it is.
const NAME: &str = "Keychron mouse";
/// What every Keychron dongle's name has (`Link-KM`, `Ultra-Link 8K`).
const DONGLE_WORD: &str = "Link";
/// A mouse's id, on its dongle and on its cable.
const ID: &str = "keychron/mouse";

/// Whether `node` is the mice's vendor interface.
pub fn reads(node: &Node) -> bool {
    has_usage(&node.descriptor, VENDOR_PAGE, VENDOR_USAGE)
}

/// Asks the mouse behind `node` for its battery until stopped or the
/// node goes, listing it while it answers.
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
        // Anything said until then (Keychron Launcher's traffic) is ignored.
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

/// The first status answer until `until`, if one comes; `Err` with why
/// reading ended when it did (`None` for a stop).
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

/// The status request.
fn request() -> Vec<u8> {
    let mut request = vec![0; 1 + REQUEST_BYTES];
    request[0] = REQUEST;
    request[1] = STATUS;
    request
}

/// One status answer's battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reading {
    percent: u8,
    charging: bool,
}

/// The battery in `report`, if it's the status answer.
fn parse(report: &[u8]) -> Option<Reading> {
    let battery = *report.get(BATTERY_AT)?;
    let reading = Reading { percent: battery & !CHARGING, charging: battery & CHARGING != 0 };
    (report[0] == ANSWER && report[1] == STATUS && reading.percent <= FULL).then_some(reading)
}

/// The mouse reached through `node`: its dongle, or its own cable.
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

/// `name` with its first word once: the kernel puts the maker before the
/// product's own name, which has it already (`Keychron Keychron M6`).
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
        // On its cable, charging.
        let charging = answer.replacen("1e5e", "1ee2", 1);
        assert_eq!(parse(&hex(&charging)), Some(Reading { percent: 98, charging: true }));
        // Another command's answer, a cut-off one, a percent past 100.
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
