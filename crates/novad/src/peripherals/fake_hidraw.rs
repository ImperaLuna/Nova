//! A stand-in for Logitech receivers' hidraw nodes in tests, answering
//! from `recorded/hidpp*.json`: what a real receiver (and its devices)
//! answered to each request, captured on the desktop. Its `/sys` entry is
//! made as the kernel makes it (`class/hidraw/<node>/device` leading to the
//! HID device, with its `driver` link and `report_descriptor`), and opening
//! the node gives one end of a `SOCK_SEQPACKET` pair, which keeps reports
//! apart as hidraw does: each write is one report, each read one.
//!
//! A request nobody recorded is answered as a receiver answers one for an
//! empty slot: HID++ 1.0's error `0x09`.

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
    /// Its report descriptor, in hex.
    pub descriptor: String,
    pub exchanges: Vec<Exchange>,
    /// Reports its devices sent by themselves, by what happened.
    #[serde(default)]
    pub notices: HashMap<String, String>,
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

/// The fake nodes, and the answers they give.
#[derive(Clone, Default)]
pub struct FakeHidraw {
    answers: Arc<Mutex<Answers>>,
    /// The node files made, and their open ends.
    nodes: Arc<Mutex<HashMap<PathBuf, Vec<std::fs::File>>>>,
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
        let class = sys.join("class/hidraw").join(&recording.node);
        std::fs::create_dir_all(&class).unwrap();
        symlink(&device, class.join("device")).unwrap();
        std::fs::create_dir_all(dev).unwrap();
        let node = dev.join(&recording.node);
        std::fs::write(&node, "").unwrap();
        lock(&self.nodes).insert(node, Vec::new());
        let mut answers = lock(&self.answers);
        for exchange in &recording.exchanges {
            answers.insert(hex(&exchange.request), exchange.replies.iter().map(|reply| hex(reply)).collect());
        }
    }

    /// From now on, `request` is answered with `replies`.
    pub fn answer(&self, request: &str, replies: &[&str]) {
        lock(&self.answers).insert(hex(request), replies.iter().map(|reply| hex(reply)).collect());
    }

    /// Every open node says `report` by itself.
    pub fn say(&self, report: &str) {
        for ends in lock(&self.nodes).values_mut() {
            for end in ends {
                let _ = end.write_all(&hex(report));
            }
        }
    }

    /// Every open node hangs up, as hidraw does when its device is
    /// unplugged.
    pub fn unplug(&self) {
        for ends in lock(&self.nodes).values_mut() {
            for end in ends.drain(..) {
                // SAFETY: `end` is an open socket for the call.
                unsafe { libc::shutdown(end.as_raw_fd(), libc::SHUT_RDWR) };
            }
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
        let ends = nodes.get_mut(path).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
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
        ends.push(served.try_clone()?);
        let answers = Arc::clone(&self.answers);
        std::thread::spawn(move || {
            let mut request = [0u8; 64];
            while let Ok(read) = served.read(&mut request) {
                if read == 0 {
                    return;
                }
                let request = &request[..read];
                let replies = lock(&answers).get(request).cloned().unwrap_or_else(|| vec![no_device(request)]);
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
