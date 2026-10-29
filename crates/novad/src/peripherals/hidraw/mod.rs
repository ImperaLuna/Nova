//! What the sources that talk to devices over hidraw themselves (HID++ for
//! Logitech's receivers, Keychron's raw HID) share: finding their nodes,
//! one reader thread on each, and gathering what the readers find.
//!
//! Nodes are found under `/sys/class/hidraw`: those of the protocol's USB
//! vendor that its [`Protocol::reads`] takes, from the HID device's ids,
//! driver, name and report descriptor. They're looked for again every
//! [`LOOK_AGAIN`] and when hidraw nodes come or go in `/dev`. Each node
//! has a reader thread of its own ([`Protocol::read`]), which ends when
//! the node goes or the source stops; one that ended by itself is started
//! again on the next look, and why it ended is logged once while that
//! stays the same.
//!
//! Opening a node needs read and write access: Nova's udev rules give it
//! to the logged-in user for the vendors read here. Without it, nothing is
//! listed and the problem is logged once.

pub mod channel;
pub mod descriptor;

use super::{Found, LOG_SOURCE, Report, Running, Source, UsbId, hotplug};
use crate::inotify::Stopper;
use crate::lock;
use crate::logger::Logger;
use channel::Channel;
use nova_protocol::LogLevel;
use std::collections::{BTreeMap, HashMap};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often nodes are looked for, besides when they come or go: one whose
/// reader failed (no access yet) is tried again.
pub const LOOK_AGAIN: Duration = Duration::from_secs(60);
/// How long after hidraw nodes come or go they're looked for: udev needs a
/// moment to give the user access.
const HOTPLUG_SETTLE: Duration = Duration::from_secs(1);
/// What hidraw nodes in `/dev` are named.
const NODE_PREFIX: &str = "hidraw";
/// The `uevent` line with a HID device's name.
const HID_NAME_KEY: &str = "HID_NAME=";

/// Opens a hidraw node for reading and writing.
pub type Opener = Arc<dyn Fn(&Path) -> std::io::Result<OwnedFd> + Send + Sync>;

/// The machine's: `open(2)` on the node.
pub fn open_node(path: &Path) -> std::io::Result<OwnedFd> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(path)?;
    Ok(file.into())
}

/// A hidraw node, as `/sys` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Its name, `hidraw17`.
    pub name: String,
    /// The HID bus (`BUS_USB`, `BUS_BLUETOOTH`) and USB ids it's on.
    pub bus: u16,
    pub id: UsbId,
    /// The kernel driver its HID device has, `hid-generic`.
    pub driver: String,
    /// Its HID device's name, `Keychron K2 HE`.
    pub hid_name: Option<String>,
    pub descriptor: Vec<u8>,
}

/// A device protocol spoken over hidraw nodes.
pub trait Protocol: Send + Sync + 'static {
    /// The USB vendor whose nodes it reads: nodes of any other are never
    /// opened, so no other maker's device is sent a report it doesn't
    /// expect.
    const VENDOR: u16;
    /// What it's called in the log.
    const NAME: &'static str;
    /// Whether it reads `node`.
    fn reads(&self, node: &Node) -> bool;
    /// Reads `node` until stopped or the node goes, telling `listing` what
    /// it finds; says why it ended when that wasn't a stop.
    fn read(&self, node: &Node, channel: Channel, listing: &Listing) -> Option<String>;
}

/// A source reading the nodes `protocol` takes.
pub struct Hidraw<P> {
    /// `/sys`, where hidraw nodes are listed.
    pub sys_root: PathBuf,
    /// `/dev`, where the nodes are.
    pub dev: PathBuf,
    pub open: Opener,
    pub protocol: P,
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

/// The devices every node's reader has found, by node, and when it last
/// said so.
struct Devices {
    by_node: Mutex<BTreeMap<String, (Instant, Vec<Found>)>>,
    report: Report,
}

impl Devices {
    /// Node `name` has `found` now. The merged list is sent on even when
    /// `found` is what the node had: hearing it again can make that node
    /// the newest for a device another node reaches too. An unchanged
    /// list goes no further.
    fn set(&self, name: &str, found: Vec<Found>) {
        let mut by_node = lock(&self.by_node);
        if found.is_empty() {
            if by_node.remove(name).is_none() {
                return;
            }
        } else {
            by_node.insert(name.to_owned(), (Instant::now(), found));
        }
        // A device two nodes reach (its cable and its receiver) once, as
        // the node that heard it last has it.
        let mut newest_first: Vec<&(Instant, Vec<Found>)> = by_node.values().collect();
        newest_first.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        let mut all: Vec<Found> = Vec::new();
        for device in newest_first.into_iter().flat_map(|(_, found)| found) {
            if !all.iter().any(|known| known.device.id == device.device.id) {
                all.push(device.clone());
            }
        }
        self.report.found(all);
    }
}

/// Where one node's reader says what it found.
pub struct Listing {
    node: String,
    devices: Arc<Devices>,
}

impl Listing {
    /// The node has `found` now (all of it).
    pub fn set(&self, found: Vec<Found>) {
        self.devices.set(&self.node, found);
    }

    pub fn logger(&self) -> &Logger {
        self.devices.report.logger()
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

impl<P: Protocol> Source for Hidraw<P> {
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
        let source = Arc::new(*self);
        std::thread::spawn(move || source.find_until_stopped(&finder, &devices));
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

impl<P: Protocol> Hidraw<P> {
    /// Looks for nodes now, then every [`LOOK_AGAIN`] and after nodes come
    /// or go, keeping one reader on each, until stopped.
    fn find_until_stopped(self: &Arc<Self>, waker: &Waker, devices: &Arc<Devices>) {
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
            next_look = Instant::now() + LOOK_AGAIN;
            let nodes = self.nodes();
            let logger = devices.report.logger();
            // Readers that ended by themselves, and those whose node went
            // or changed (another device got its name).
            for (name, reader) in std::mem::take(&mut readers) {
                if !reader.thread.is_finished() && nodes.contains(&reader.node) {
                    readers.insert(name, reader);
                    continue;
                }
                reader.stop.stop();
                // One that ended because its device was unplugged has
                // nothing to say.
                let still_there = nodes.iter().any(|node| node.name == name);
                if let Ok(Some(problem)) = reader.thread.join()
                    && still_there
                {
                    note_problem::<P>(&mut problems, &name, problem, logger);
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
                    Err(problem) => note_problem::<P>(&mut problems, &node.name, problem, logger),
                }
            }
        }
    }

    fn start_reader(self: &Arc<Self>, node: &Node, devices: &Arc<Devices>) -> Result<Reader, String> {
        let fd = (self.open)(&self.dev.join(&node.name)).map_err(|e| format!("can't open it ({e})"))?;
        let stop = Stopper::new().map_err(|e| e.to_string())?;
        let channel = Channel::new(fd, stop.clone());
        let reading = node.clone();
        let source = Arc::clone(self);
        let listing = Listing { node: node.name.clone(), devices: Arc::clone(devices) };
        let thread = std::thread::spawn(move || {
            let ended = source.protocol.read(&reading, channel, &listing);
            listing.set(Vec::new());
            ended
        });
        Ok(Reader { node: node.clone(), stop, thread })
    }

    /// The nodes the protocol reads now.
    fn nodes(&self) -> Vec<Node> {
        let Ok(entries) = std::fs::read_dir(self.sys_root.join("class/hidraw")) else { return Vec::new() };
        let mut nodes: Vec<Node> = entries
            .flatten()
            .filter_map(|entry| node(&entry.path(), P::VENDOR))
            .filter(|node| self.protocol.reads(node))
            .collect();
        nodes.sort_by(|a, b| a.name.cmp(&b.name));
        nodes
    }
}

/// Logs node `name`'s `problem` unless it's the one it had last.
fn note_problem<P: Protocol>(problems: &mut HashMap<String, String>, name: &str, problem: String, logger: &Logger) {
    if problems.get(name) != Some(&problem) {
        logger.warn(LOG_SOURCE, &format!("no batteries from {} node {name}: {problem}", P::NAME));
        problems.insert(name.to_owned(), problem);
    }
}

/// The node at `/sys/class/hidraw/<name>`, if its device is `vendor`'s.
fn node(entry: &Path, vendor: u16) -> Option<Node> {
    let name = entry.file_name()?.to_str()?.to_owned();
    let device = entry.join("device").canonicalize().ok()?;
    let (bus, id) = hid_id(device.file_name()?.to_str()?)?;
    if id.vendor != vendor {
        return None;
    }
    let driver = std::fs::read_link(device.join("driver")).ok()?.file_name()?.to_str()?.to_owned();
    let hid_name = std::fs::read_to_string(device.join("uevent"))
        .ok()
        .and_then(|uevent| uevent.lines().find_map(|line| line.strip_prefix(HID_NAME_KEY)).and_then(super::name));
    let descriptor = std::fs::read(device.join("report_descriptor")).ok()?;
    Some(Node { name, bus, id, driver, hid_name, descriptor })
}

/// The bus and ids in a HID device's folder name, `BBBB:VVVV:PPPP.NNNN`.
fn hid_id(name: &str) -> Option<(u16, UsbId)> {
    let (ids, _instance) = name.split_once('.')?;
    let mut parts = ids.split(':');
    let mut next = || parts.next().filter(|part| part.len() == 4).and_then(|part| u16::from_str_radix(part, 16).ok());
    let (bus, vendor, product) = (next()?, next()?, next()?);
    parts.next().is_none().then_some((bus, UsbId { vendor, product }))
}
