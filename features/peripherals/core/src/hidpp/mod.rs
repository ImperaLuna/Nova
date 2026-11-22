//! Logitech devices whose receiver the kernel doesn't drive, read over
//! HID++ like Solaar does. A receiver the kernel's Logitech drivers don't
//! know yet (Lightspeed `c547`, Bolt `c548` on many kernels) stays on
//! `hid-generic`, so its devices' batteries never reach `/sys` or UPower.
//! This source opens the HID++ hidraw node of such a receiver (or of a
//! device on its own cable) and asks its devices itself.
//!
//! Nodes are found as [`super::hidraw`] finds them: those whose HID device
//! is on `hid-generic` (one a Logitech driver has is UPower's to report)
//! and whose report descriptor has HID++ reports. Each node's reader asks
//! a receiver's paired devices (or the device itself) whether they're
//! there, what they are and how charged, again every [`POLL`] and shortly
//! after a device says something by itself (it connected, its battery
//! changed).
//!
//! Opening a node needs read and write access: Nova's udev rule gives it
//! to the logged-in user for Logitech's nodes. Without it, nothing is listed and
//! the problem is logged once.

mod descriptor;
mod device;
mod protocol;

use super::hidraw::channel::Channel;
use super::hidraw::{Listing, Node, Protocol};
use super::{Found, LOG_SOURCE, UsbId};
use device::{Conversation, Failure, Info, Ping};
use nova_carrier::log::LogLevel;
use nova_peripherals_api::{Peripheral, PeripheralLink, PeripheralSource};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// How often devices are asked again: a device turned on or off says
/// nothing to `/dev`.
pub const POLL: Duration = Duration::from_secs(60);
/// How long after a device says something by itself it's asked again:
/// one that just connected needs a moment.
const NOTICE_SETTLE: Duration = Duration::from_secs(1);
/// How soon a device that didn't answer is asked again, and how many reads
/// in a row may fail before it leaves the list.
const RETRY: Duration = Duration::from_secs(5);
const MAX_MISSES: u32 = 3;
/// The driver a HID device has when no other claimed it.
const GENERIC_DRIVER: &str = "hid-generic";
/// HID's bus numbers (`BUS_USB`, `BUS_BLUETOOTH` in linux/input.h).
const HID_BUS_USB: u16 = 0x0003;
const HID_BUS_BLUETOOTH: u16 = 0x0005;
/// Logitech's USB vendor id, which its devices' product ids are under.
const LOGITECH: u16 = 0x046D;
/// The id a device's comes after.
const ID_PREFIX: &str = "hidpp/";
/// HID++ 1.0's notification that a paired device connected or went.
const CONNECTION_NOTICE: u8 = 0x41;

/// HID++, on Logitech's nodes that are on `hid-generic` (one a Logitech
/// driver has is UPower's to report) and take HID++ reports.
pub struct Hidpp;

impl Protocol for Hidpp {
    const VENDOR: u16 = LOGITECH;
    const NAME: &'static str = "HID++";

    fn reads(&self, node: &Node) -> bool {
        node.driver == GENERIC_DRIVER && descriptor::reports(&node.descriptor).any()
    }

    fn read(&self, node: &Node, channel: Channel, listing: &Listing) -> Option<String> {
        let talk = Conversation::new(channel, descriptor::reports(&node.descriptor));
        read_node(node, talk, listing)
    }
}

/// Asks the devices on `node` until stopped or the node goes; says why it
/// ended when that wasn't a stop.
fn read_node(node: &Node, mut talk: Conversation, listing: &Listing) -> Option<String> {
    let ended = |failure: Failure| match failure {
        Failure::Stopped => None,
        Failure::Io(e) => Some(e.to_string()),
        other => Some(format!("{other:?}")),
    };
    let indices: Vec<u8> = match talk.ping(protocol::OWN_INDEX) {
        Ok(Ping::There) => vec![protocol::OWN_INDEX],
        Ok(Ping::Receiver) => {
            match talk.follow_connections() {
                Err(failure @ (Failure::Stopped | Failure::Io(_))) => return ended(failure),
                Err(e) => listing.logger().log(
                    LogLevel::Debug,
                    LOG_SOURCE,
                    &format!("{}: receiver won't say when devices connect ({e:?}); read every minute", node.name),
                ),
                Ok(()) => {}
            }
            protocol::PAIRED_INDICES.collect()
        }
        Ok(Ping::Gone) | Err(Failure::NoAnswer) => return Some("it doesn't answer HID++".into()),
        Err(failure) => return ended(failure),
    };
    let mut known: HashMap<u8, Info> = HashMap::new();
    let mut found: BTreeMap<u8, Found> = BTreeMap::new();
    // Reads in a row that failed, by device.
    let mut misses: HashMap<u8, u32> = HashMap::new();
    let mut due: BTreeMap<u8, Instant> = BTreeMap::new();
    let mut next_poll = Instant::now();
    loop {
        let now = Instant::now();
        if now >= next_poll {
            due.extend(indices.iter().map(|index| (*index, now)));
            next_poll = now + POLL;
        }
        let ready: Vec<u8> = due.iter().filter(|(_, at)| **at <= now).map(|(index, _)| *index).collect();
        for index in &ready {
            due.remove(index);
            match read_device(&mut talk, node, *index, &mut known) {
                Ok(read) => {
                    misses.remove(index);
                    match read {
                        Some(device) => found.insert(*index, device),
                        None => found.remove(index),
                    };
                }
                Err(failure @ (Failure::Stopped | Failure::Io(_))) => return ended(failure),
                // A slow or lost answer over the air: its last reading
                // stays while it's asked again soon, a few times.
                Err(_) => {
                    let missed = misses.entry(*index).or_default();
                    *missed += 1;
                    if *missed < MAX_MISSES {
                        due.insert(*index, Instant::now() + RETRY);
                    } else {
                        misses.remove(index);
                        known.remove(index);
                        found.remove(index);
                    }
                }
            }
        }
        if !ready.is_empty() {
            listing.set(found.values().cloned().collect());
        }
        for report in std::mem::take(&mut talk.heard) {
            noticed(&report, &indices, &mut due, &mut known);
        }
        let until = due.values().copied().min().map_or(next_poll, |at| at.min(next_poll));
        match talk.wait(until) {
            Ok(Some(report)) => noticed(&report, &indices, &mut due, &mut known),
            Ok(None) => {}
            Err(failure) => return ended(failure),
        }
    }
}

/// A device said something by itself: it's asked again shortly, and read
/// whole when it (re)connected, since another device may have its index.
fn noticed(report: &[u8], indices: &[u8], due: &mut BTreeMap<u8, Instant>, known: &mut HashMap<u8, Info>) {
    let Some(index) = protocol::unasked(report).filter(|index| indices.contains(index)) else { return };
    if report[2] == CONNECTION_NOTICE {
        known.remove(&index);
    }
    let at = Instant::now() + NOTICE_SETTLE;
    due.entry(index).and_modify(|due| *due = (*due).min(at)).or_insert(at);
}

/// Device `index` on `node`, if it's there and has a battery.
fn read_device(
    talk: &mut Conversation,
    node: &Node,
    index: u8,
    known: &mut HashMap<u8, Info>,
) -> Result<Option<Found>, Failure> {
    if talk.ping(index)? != Ping::There {
        known.remove(&index);
        return Ok(None);
    }
    let info = match known.get(&index) {
        Some(info) => info.clone(),
        None => {
            let info = talk.info(index)?;
            known.insert(index, info.clone());
            info
        }
    };
    let Some(feature) = info.battery else { return Ok(None) };
    let charge = talk.charge(index, feature)?;
    if charge.battery.is_none() {
        return Ok(None);
    }
    let paired = index != protocol::OWN_INDEX;
    let link = match node.bus {
        HID_BUS_BLUETOOTH => PeripheralLink::Bluetooth,
        HID_BUS_USB if paired => PeripheralLink::Dongle,
        _ => PeripheralLink::Wired,
    };
    let mut usb_ids: Vec<UsbId> =
        info.product_ids.iter().map(|product| UsbId { vendor: LOGITECH, product: *product }).collect();
    if !paired && !usb_ids.contains(&node.id) {
        usb_ids.push(node.id);
    }
    let id = match info.unit_id {
        Some(unit) => format!("{ID_PREFIX}{}", unit.iter().map(|byte| format!("{byte:02x}")).collect::<String>()),
        None => format!("{ID_PREFIX}{}/{index}", node.id),
    };
    let name = info.name.clone().unwrap_or_else(|| usb_ids.first().unwrap_or(&node.id).to_string());
    let device = Peripheral {
        id,
        name,
        kind: info.kind,
        link: Some(link),
        battery: charge.battery,
        charging: charge.charging,
        source: PeripheralSource::Hidpp,
    };
    // As the kernel's driver gives it (`df-70-97-d8`), and UPower then, so
    // two of one model aren't taken for one.
    let serial = info.unit_id.map(|unit| unit.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join("-"));
    Ok(Some(Found { device, usb_ids, serial }))
}
