//! Logitech devices whose receiver the kernel doesn't drive, read over
//! HID++ like Solaar does. A receiver the kernel's Logitech drivers don't
//! know yet (Lightspeed `c547`, Bolt `c548` on many kernels) stays on
//! `hid-generic`, so its devices' batteries never reach `/sys` or UPower.
//! This source opens the HID++ hidraw node of such a receiver (or of a
//! device on its own cable) and asks its devices itself.
//!
//! Nodes are found under `/sys/class/hidraw`: those whose HID device is on
//! `hid-generic` (one a Logitech driver has is UPower's to report) and
//! whose report descriptor has HID++ reports. They're looked for again
//! every [`POLL`] and when hidraw nodes come or go in `/dev`. Each node
//! has a reader thread of its own, which asks a receiver's paired devices
//! (or the device itself) whether they're there, what they are and how
//! charged, again every [`POLL`] and shortly after a device says
//! something by itself (it connected, its battery changed).
//!
//! Opening a node needs read and write access: Nova's udev rule gives it
//! to the logged-in user for Logitech's. Without it, nothing is listed and
//! the problem is logged once.

mod channel;
mod descriptor;
mod device;
mod protocol;

use super::{Found, LOG_SOURCE, Report, Running, Source, UsbId, hotplug};
use crate::inotify::Stopper;
use crate::lock;
use channel::Channel;
use descriptor::Reports;
use device::{Conversation, Failure, Info, Ping};
use nova_protocol::{LogLevel, Peripheral, PeripheralLink, PeripheralSource};
use std::collections::{BTreeMap, HashMap};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often devices are asked again, and nodes looked for: a device
/// turned on or off says nothing to `/dev`.
pub const POLL: Duration = Duration::from_secs(60);
/// How long after hidraw nodes come or go they're looked for: udev needs a
/// moment to give the user access.
const HOTPLUG_SETTLE: Duration = Duration::from_secs(1);
/// How long after a device says something by itself it's asked again:
/// one that just connected needs a moment.
const NOTICE_SETTLE: Duration = Duration::from_secs(1);
/// How soon a device that didn't answer is asked again, and how many reads
/// in a row may fail before it leaves the list.
const RETRY: Duration = Duration::from_secs(5);
const MAX_MISSES: u32 = 3;
/// What hidraw nodes in `/dev` are named.
const NODE_PREFIX: &str = "hidraw";
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

/// Opens a hidraw node for reading and writing.
pub type Opener = Arc<dyn Fn(&Path) -> std::io::Result<OwnedFd> + Send + Sync>;

/// The machine's: `open(2)` on the node.
pub fn open_node(path: &Path) -> std::io::Result<OwnedFd> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(path)?;
    Ok(file.into())
}

pub struct Hidpp {
    /// `/sys`, where hidraw nodes are listed.
    pub sys_root: PathBuf,
    /// `/dev`, where the nodes are.
    pub dev: PathBuf,
    pub open: Opener,
}

/// A HID++ hidraw node the kernel left to us.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    /// Its name, `hidraw17`.
    name: String,
    /// The HID bus and USB ids it's on.
    bus: u16,
    id: UsbId,
    reports: Reports,
}

/// What wakes the finder.
#[derive(Default)]
struct Wakes {
    stopped: bool,
    plugged: Option<Instant>,
}

#[derive(Default)]
struct Waker {
    wakes: Mutex<Wakes>,
    condvar: Condvar,
}

/// The devices every node's reader has found, by node.
struct Devices {
    by_node: Mutex<BTreeMap<String, Vec<Found>>>,
    report: Report,
}

impl Devices {
    /// Node `name` has `found` now.
    fn set(&self, name: &str, found: Vec<Found>) {
        let mut by_node = lock(&self.by_node);
        let changed = match by_node.get(name) {
            Some(known) => *known != found,
            None => !found.is_empty(),
        };
        if found.is_empty() {
            by_node.remove(name);
        } else {
            by_node.insert(name.to_owned(), found);
        }
        if changed {
            // A device two nodes reach (its cable and its receiver) once.
            let mut all: Vec<Found> = Vec::new();
            for device in by_node.values().flatten() {
                if !all.iter().any(|known| known.device.id == device.device.id) {
                    all.push(device.clone());
                }
            }
            self.report.found(all);
        }
    }
}

struct Finding {
    waker: Arc<Waker>,
    hotplug: Option<Stopper>,
}

impl Running for Finding {
    fn stop(&self) {
        lock(&self.waker.wakes).stopped = true;
        self.waker.condvar.notify_all();
        if let Some(hotplug) = &self.hotplug {
            hotplug.stop();
        }
    }
}

impl Source for Hidpp {
    fn start(self: Box<Self>, report: Report) -> Box<dyn Running> {
        let waker = Arc::new(Waker::default());
        let plugged = Arc::clone(&waker);
        let failed = report.clone();
        let hotplug = hotplug::watch_entries(
            &self.dev,
            NODE_PREFIX,
            move || {
                lock(&plugged.wakes).plugged = Some(Instant::now());
                plugged.condvar.notify_all();
            },
            move |e| failed.logger().warn(LOG_SOURCE, &format!("stopped following hidraw nodes: {e}")),
        )
        .map_err(|e| {
            let problem = format!("can't follow hidraw nodes in {} ({e})", self.dev.display());
            report.logger().log(LogLevel::Debug, LOG_SOURCE, &format!("{problem}; they're looked for every minute"));
        })
        .ok();
        let finder = Arc::clone(&waker);
        let devices = Arc::new(Devices { by_node: Mutex::default(), report });
        std::thread::spawn(move || self.find_until_stopped(&finder, &devices));
        Box::new(Finding { waker, hotplug })
    }
}

/// A node's running reader.
struct Reader {
    node: Node,
    stop: Stopper,
    /// Ends with why it stopped, when it did by itself.
    thread: JoinHandle<Option<String>>,
}

impl Hidpp {
    /// Looks for nodes now, then every [`POLL`] and after nodes come or
    /// go, keeping one reader on each, until stopped.
    fn find_until_stopped(&self, waker: &Waker, devices: &Arc<Devices>) {
        let mut readers: HashMap<String, Reader> = HashMap::new();
        // The last problem each node had, so a lasting one is logged once.
        let mut problems: HashMap<String, String> = HashMap::new();
        let mut next_look = Instant::now();
        loop {
            {
                let mut wakes = lock(&waker.wakes);
                loop {
                    if wakes.stopped {
                        for reader in readers.values() {
                            reader.stop.stop();
                        }
                        return;
                    }
                    let due = wakes.plugged.map_or(next_look, |plugged| next_look.min(plugged + HOTPLUG_SETTLE));
                    let now = Instant::now();
                    if now >= due {
                        break;
                    }
                    wakes = waker
                        .condvar
                        .wait_timeout(wakes, due - now)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0;
                }
                wakes.plugged = None;
            }
            next_look = Instant::now() + POLL;
            let nodes = self.nodes();
            let logger = devices.report.logger();
            // Readers that ended by themselves, and those whose node went
            // or changed (another receiver got its name).
            for (name, reader) in std::mem::take(&mut readers) {
                if !reader.thread.is_finished() && nodes.contains(&reader.node) {
                    readers.insert(name, reader);
                    continue;
                }
                reader.stop.stop();
                // One that ended because its receiver was unplugged has
                // nothing to say.
                let still_there = nodes.iter().any(|node| node.name == name);
                if let Ok(Some(problem)) = reader.thread.join()
                    && still_there
                {
                    note_problem(&mut problems, &name, problem, logger);
                }
            }
            problems.retain(|name, _| nodes.iter().any(|node| node.name == *name));
            for node in nodes {
                if readers.contains_key(&node.name) {
                    continue;
                }
                match self.start_reader(&node, devices) {
                    Ok(reader) => {
                        problems.remove(&node.name);
                        readers.insert(node.name.clone(), reader);
                    }
                    Err(problem) => note_problem(&mut problems, &node.name, problem, logger),
                }
            }
        }
    }

    fn start_reader(&self, node: &Node, devices: &Arc<Devices>) -> Result<Reader, String> {
        let fd = (self.open)(&self.dev.join(&node.name)).map_err(|e| format!("can't open it ({e})"))?;
        let stop = Stopper::new().map_err(|e| e.to_string())?;
        let conversation = Conversation::new(Channel::new(fd, stop.clone()), node.reports);
        let reading = node.clone();
        let devices = Arc::clone(devices);
        let thread = std::thread::spawn(move || {
            let ended = read_node(&reading, conversation, &devices);
            devices.set(&reading.name, Vec::new());
            ended
        });
        Ok(Reader { node: node.clone(), stop, thread })
    }

    /// Logitech's HID++ nodes on `hid-generic` now.
    fn nodes(&self) -> Vec<Node> {
        let Ok(entries) = std::fs::read_dir(self.sys_root.join("class/hidraw")) else { return Vec::new() };
        let mut nodes: Vec<Node> = entries.flatten().filter_map(|entry| node(&entry.path())).collect();
        nodes.sort_by(|a, b| a.name.cmp(&b.name));
        nodes
    }
}

/// Logs node `name`'s `problem` unless it's the one it had last.
fn note_problem(problems: &mut HashMap<String, String>, name: &str, problem: String, logger: &crate::logger::Logger) {
    if problems.get(name) != Some(&problem) {
        logger.warn(LOG_SOURCE, &format!("no batteries from HID++ node {name}: {problem}"));
        problems.insert(name.to_owned(), problem);
    }
}

/// The node at `/sys/class/hidraw/<name>`, if it's Logitech's, on
/// `hid-generic` and takes HID++ reports. Only Logitech speaks HID++, and
/// another maker's device is never sent a report it doesn't expect.
fn node(entry: &Path) -> Option<Node> {
    let name = entry.file_name()?.to_str()?.to_owned();
    let device = entry.join("device").canonicalize().ok()?;
    let driver = std::fs::read_link(device.join("driver")).ok()?;
    if driver.file_name()? != GENERIC_DRIVER {
        return None;
    }
    let (bus, id) = hid_id(device.file_name()?.to_str()?)?;
    if id.vendor != LOGITECH {
        return None;
    }
    let reports = descriptor::reports(&std::fs::read(device.join("report_descriptor")).ok()?);
    reports.any().then_some(Node { name, bus, id, reports })
}

/// The bus and ids in a HID device's folder name, `BBBB:VVVV:PPPP.NNNN`.
fn hid_id(name: &str) -> Option<(u16, UsbId)> {
    let (ids, _instance) = name.split_once('.')?;
    let mut parts = ids.split(':');
    let mut next = || parts.next().filter(|part| part.len() == 4).and_then(|part| u16::from_str_radix(part, 16).ok());
    let (bus, vendor, product) = (next()?, next()?, next()?);
    parts.next().is_none().then_some((bus, UsbId { vendor, product }))
}

/// Asks the devices on `node` until stopped or the node goes; says why it
/// ended when that wasn't a stop.
fn read_node(node: &Node, mut talk: Conversation, devices: &Devices) -> Option<String> {
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
                Err(e) => devices.report.logger().log(
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
            devices.set(&node.name, found.values().cloned().collect());
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
