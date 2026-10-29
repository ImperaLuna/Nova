//! A stand-in for hidraw nodes in tests, answering from
//! `recorded/hidpp*.json` and `recorded/keychron*.json`: what a real
//! receiver or dongle (and its devices) answered to each request, and
//! said by itself, captured on the desktop. Its `/sys` entry is made as
//! the kernel makes it (`class/hidraw/<node>/device` leading to the HID
//! device, with its `driver` link, `uevent` and `report_descriptor`), and
//! opening the node gives one end of a `SOCK_SEQPACKET` pair, which keeps
//! reports apart as hidraw does: each write is one report, each read one.
//!
//! A request nobody recorded is answered as the recording says: as a
//! Logitech receiver answers one for an empty slot (HID++ 1.0's error
//! `0x09`), or not at all, as a Keychron dongle.

use super::hidraw::Opener;
use crate::lock;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// HID++ 1.0's error for a slot with no device in it.
const NO_DEVICE: u8 = 0x09;

/// A `recorded/hidpp*.json`.
#[derive(Deserialize)]
pub struct Recording {
    /// The hidraw node, `hidraw17`.
    pub node: String,
    /// Its HID device's folder, `0003:046D:C547.0007`.
    pub hid: String,
    pub driver: String,
    /// Its HID device's name, in its `uevent`.
    #[serde(default)]
    pub hid_name: Option<String>,
    /// Its report descriptor, in hex.
    pub descriptor: String,
    #[serde(default)]
    pub exchanges: Vec<Exchange>,
    /// How a request nobody recorded is answered.
    #[serde(default)]
    pub unrecorded: Unrecorded,
    /// Reports its devices sent by themselves, by what happened.
    #[serde(default)]
    pub notices: HashMap<String, String>,
    /// Reports its devices sent by themselves, in the order they came.
    #[serde(default)]
    pub pushes: Vec<String>,
}

/// How a node answers a request nobody recorded.
#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum Unrecorded {
    /// As a Logitech receiver for an empty slot.
    #[default]
    HidppNoDevice,
    /// It doesn't.
    Silent,
}

#[derive(Deserialize, Clone)]
pub struct Exchange {
    pub request: String,
    /// Everything the node said until the answer, the answer last.
    pub replies: Vec<String>,
}

pub fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

/// Each request's replies.
type Answers = HashMap<Vec<u8>, Vec<Vec<u8>>>;

/// One fake node.
struct Node {
    answers: Answers,
    unrecorded: Unrecorded,
    /// Its open ends.
    ends: Vec<std::fs::File>,
    /// The requests written to it, in order.
    requests: Vec<Vec<u8>>,
}

/// The fake nodes, by their file, and the answers they give.
#[derive(Clone, Default)]
pub struct FakeHidraw {
    nodes: Arc<Mutex<HashMap<PathBuf, Node>>>,
    /// The nodes opened, in order.
    pub opened: Arc<Mutex<Vec<PathBuf>>>,
}

impl FakeHidraw {
    /// Makes `recording`'s node under `sys` and `dev`.
    pub fn add(&self, sys: &Path, dev: &Path, recording: &Recording) {
        let device = sys.join("devices/pci0000:00/usb1/1-1/1-1:1.2").join(&recording.hid);
        let driver = sys.join("bus/hid/drivers").join(&recording.driver);
        std::fs::create_dir_all(&device).unwrap();
        std::fs::create_dir_all(&driver).unwrap();
        symlink(&driver, device.join("driver")).unwrap();
        std::fs::write(device.join("report_descriptor"), hex(&recording.descriptor)).unwrap();
        if let Some(name) = &recording.hid_name {
            std::fs::write(device.join("uevent"), format!("DRIVER={}\nHID_NAME={name}\n", recording.driver)).unwrap();
        }
        let class = sys.join("class/hidraw").join(&recording.node);
        std::fs::create_dir_all(&class).unwrap();
        symlink(&device, class.join("device")).unwrap();
        std::fs::create_dir_all(dev).unwrap();
        let node = dev.join(&recording.node);
        std::fs::write(&node, "").unwrap();
        let answers = recording
            .exchanges
            .iter()
            .map(|exchange| (hex(&exchange.request), exchange.replies.iter().map(|reply| hex(reply)).collect()))
            .collect();
        lock(&self.nodes)
            .insert(node, Node { answers, unrecorded: recording.unrecorded, ends: Vec::new(), requests: Vec::new() });
    }

    /// From now on, `request` is answered with `replies` on every node.
    pub fn answer(&self, request: &str, replies: &[&str]) {
        for node in lock(&self.nodes).values_mut() {
            node.answers.insert(hex(request), replies.iter().map(|reply| hex(reply)).collect());
        }
    }

    /// Every open node says `report` by itself.
    pub fn say(&self, report: &str) {
        for node in lock(&self.nodes).values_mut() {
            for end in &mut node.ends {
                let _ = end.write_all(&hex(report));
            }
        }
    }

    /// Node `path`, if open, says `report` by itself.
    pub fn say_on(&self, path: &Path, report: &str) {
        for end in lock(&self.nodes).get_mut(path).map(|node| node.ends.as_mut_slice()).unwrap_or_default() {
            let _ = end.write_all(&hex(report));
        }
    }

    /// The requests written to node `path` so far.
    pub fn requests(&self, path: &Path) -> Vec<Vec<u8>> {
        lock(&self.nodes).get(path).map(|node| node.requests.clone()).unwrap_or_default()
    }

    /// Node `path` hangs up, as hidraw does when its device is unplugged.
    pub fn hang_up(&self, path: &Path) {
        let ends: Vec<std::fs::File> =
            lock(&self.nodes).get_mut(path).map(|node| std::mem::take(&mut node.ends)).unwrap_or_default();
        for end in ends {
            // SAFETY: `end` is an open socket for the call.
            unsafe { libc::shutdown(end.as_raw_fd(), libc::SHUT_RDWR) };
        }
    }

    /// Every open node hangs up, as hidraw does when its device is
    /// unplugged.
    pub fn unplug(&self) {
        let paths: Vec<PathBuf> = lock(&self.nodes).keys().cloned().collect();
        for path in paths {
            self.hang_up(&path);
        }
    }

    /// Opens the fake nodes; any other path isn't there.
    pub fn opener(&self) -> Opener {
        let fake = self.clone();
        Arc::new(move |path: &Path| fake.open(path))
    }

    fn open(&self, path: &Path) -> std::io::Result<OwnedFd> {
        lock(&self.opened).push(path.to_owned());
        let mut nodes = lock(&self.nodes);
        let node = nodes.get_mut(path).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for the two descriptors returned.
        let made =
            unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr()) };
        if made == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: both were just made and nothing else owns them.
        let (ours, theirs) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        let mut served = std::fs::File::from(theirs);
        node.ends.push(served.try_clone()?);
        let nodes = Arc::clone(&self.nodes);
        let path = path.to_owned();
        std::thread::spawn(move || {
            let mut request = [0u8; 64];
            while let Ok(read) = served.read(&mut request) {
                if read == 0 {
                    return;
                }
                let request = &request[..read];
                let replies = {
                    let mut nodes = lock(&nodes);
                    let Some(node) = nodes.get_mut(&path) else { return };
                    node.requests.push(request.to_vec());
                    match (node.answers.get(request), node.unrecorded) {
                        (Some(replies), _) => replies.clone(),
                        (None, Unrecorded::HidppNoDevice) => vec![no_device(request)],
                        (None, Unrecorded::Silent) => Vec::new(),
                    }
                };
                for reply in replies {
                    if served.write_all(&reply).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(ours)
    }
}

/// A receiver's answer to a request for an empty slot.
fn no_device(request: &[u8]) -> Vec<u8> {
    vec![0x10, request[1], 0x8F, request[2], request[3], NO_DEVICE, 0]
}
