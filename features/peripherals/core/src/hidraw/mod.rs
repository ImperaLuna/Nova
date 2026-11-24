pub mod channel;
pub mod descriptor;

use super::hotplug::{self, Waker};
use super::{Found, LOG_SOURCE, Report, Running, Source, UsbId};
use channel::Channel;
use nova_base::lock;
use nova_carrier::log::LogLevel;
use nova_inotify::Stopper;
use nova_log::Logger;
use nova_peripherals_api::PeripheralLink;
use std::collections::{BTreeMap, HashMap};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const LOOK_AGAIN: Duration = Duration::from_secs(60);
const HOTPLUG_SETTLE: Duration = Duration::from_secs(1);
const NODE_PREFIX: &str = "hidraw";
const HID_NAME_KEY: &str = "HID_NAME=";

pub type Opener = Arc<dyn Fn(&Path) -> std::io::Result<OwnedFd> + Send + Sync>;

pub fn open_node(path: &Path) -> std::io::Result<OwnedFd> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(path)?;
    Ok(file.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub bus: u16,
    pub id: UsbId,
    pub driver: String,
    pub hid_name: Option<String>,
    pub descriptor: Vec<u8>,
}

pub trait Protocol: Send + Sync + 'static {
    const VENDOR: u16;
    const NAME: &'static str;
    fn reads(&self, node: &Node) -> bool;
    fn read(&self, node: &Node, channel: Channel, listing: &Listing) -> Option<String>;
}

pub struct Hidraw<P> {
    pub sys_root: PathBuf,
    pub dev: PathBuf,
    pub open: Opener,
    pub protocol: P,
}

struct Devices {
    by_node: Mutex<BTreeMap<String, (Instant, Vec<Found>)>>,
    report: Report,
}

impl Devices {
    fn set(&self, name: &str, found: Vec<Found>) {
        let mut by_node = lock(&self.by_node);
        if found.is_empty() {
            if by_node.remove(name).is_none() {
                return;
            }
        } else {
            by_node.insert(name.to_owned(), (Instant::now(), found));
        }
        let mut candidates: Vec<(&Instant, &Found)> =
            by_node.values().flat_map(|(at, found)| found.iter().map(move |device| (at, device))).collect();
        candidates
            .sort_by_key(|(at, device)| (device.device.link != Some(PeripheralLink::Wired), std::cmp::Reverse(**at)));
        let mut all: Vec<Found> = Vec::new();
        for (_, device) in candidates {
            if !all.iter().any(|known| known.device.id == device.device.id) {
                all.push(device.clone());
            }
        }
        self.report.found(all);
    }
}

pub struct Listing {
    node: String,
    devices: Arc<Devices>,
}

impl Listing {
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
        self.waker.stop();
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
            move || plugged.plugged(),
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

struct Reader {
    node: Node,
    stop: Stopper,
    thread: JoinHandle<Option<String>>,
}

impl<P: Protocol> Hidraw<P> {
    fn find_until_stopped(self: &Arc<Self>, waker: &Waker, devices: &Arc<Devices>) {
        let mut readers: HashMap<String, Reader> = HashMap::new();
        let mut problems: HashMap<String, String> = HashMap::new();
        let mut next_look = Instant::now();
        loop {
            if !waker.wait(next_look, HOTPLUG_SETTLE) {
                for reader in readers.values() {
                    reader.stop.stop();
                }
                return;
            }
            next_look = Instant::now() + LOOK_AGAIN;
            let nodes = self.nodes();
            let logger = devices.report.logger();
            for (name, reader) in std::mem::take(&mut readers) {
                if !reader.thread.is_finished() && nodes.contains(&reader.node) {
                    readers.insert(name, reader);
                    continue;
                }
                reader.stop.stop();
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

fn note_problem<P: Protocol>(problems: &mut HashMap<String, String>, name: &str, problem: String, logger: &Logger) {
    if problems.get(name) != Some(&problem) {
        logger.warn(LOG_SOURCE, &format!("no batteries from {} node {name}: {problem}", P::NAME));
        problems.insert(name.to_owned(), problem);
    }
}

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

fn hid_id(name: &str) -> Option<(u16, UsbId)> {
    let (ids, _instance) = name.split_once('.')?;
    let mut parts = ids.split(':');
    let mut next = || parts.next().filter(|part| part.len() == 4).and_then(|part| u16::from_str_radix(part, 16).ok());
    let (bus, vendor, product) = (next()?, next()?, next()?);
    parts.next().is_none().then_some((bus, UsbId { vendor, product }))
}
