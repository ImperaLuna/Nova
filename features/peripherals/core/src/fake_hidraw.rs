use super::hidraw::Opener;
use nova_base::lock;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const NO_DEVICE: u8 = 0x09;

#[derive(Deserialize)]
pub struct Recording {
    pub node: String,
    pub hid: String,
    pub driver: String,
    #[serde(default)]
    pub hid_name: Option<String>,
    pub descriptor: String,
    #[serde(default)]
    pub exchanges: Vec<Exchange>,
    #[serde(default)]
    pub unrecorded: Unrecorded,
    #[serde(default)]
    pub notices: HashMap<String, String>,
    #[serde(default)]
    pub pushes: Vec<String>,
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum Unrecorded {
    #[default]
    HidppNoDevice,
    Silent,
}

#[derive(Deserialize, Clone)]
pub struct Exchange {
    pub request: String,
    pub replies: Vec<String>,
}

pub fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

type Answers = HashMap<Vec<u8>, Vec<Vec<u8>>>;

struct Node {
    answers: Answers,
    unrecorded: Unrecorded,
    ends: Vec<std::fs::File>,
    requests: Vec<Vec<u8>>,
}

#[derive(Clone, Default)]
pub struct FakeHidraw {
    nodes: Arc<Mutex<HashMap<PathBuf, Node>>>,
    pub opened: Arc<Mutex<Vec<PathBuf>>>,
}

impl FakeHidraw {
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

    pub fn answer(&self, request: &str, replies: &[&str]) {
        for node in lock(&self.nodes).values_mut() {
            node.answers.insert(hex(request), replies.iter().map(|reply| hex(reply)).collect());
        }
    }

    pub fn say(&self, report: &str) {
        for node in lock(&self.nodes).values_mut() {
            for end in &mut node.ends {
                let _ = end.write_all(&hex(report));
            }
        }
    }

    pub fn say_on(&self, path: &Path, report: &str) {
        for end in lock(&self.nodes).get_mut(path).map(|node| node.ends.as_mut_slice()).unwrap_or_default() {
            let _ = end.write_all(&hex(report));
        }
    }

    pub fn requests(&self, path: &Path) -> Vec<Vec<u8>> {
        lock(&self.nodes).get(path).map(|node| node.requests.clone()).unwrap_or_default()
    }

    pub fn hang_up(&self, path: &Path) {
        let ends: Vec<std::fs::File> =
            lock(&self.nodes).get_mut(path).map(|node| std::mem::take(&mut node.ends)).unwrap_or_default();
        for end in ends {
            // SAFETY: `end` is an open socket for the call.
            unsafe { libc::shutdown(end.as_raw_fd(), libc::SHUT_RDWR) };
        }
    }

    pub fn unplug(&self) {
        let paths: Vec<PathBuf> = lock(&self.nodes).keys().cloned().collect();
        for path in paths {
            self.hang_up(&path);
        }
    }

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
        std::thread::spawn(move || serve(&mut served, &nodes, &path));
        Ok(ours)
    }
}

fn serve(served: &mut std::fs::File, nodes: &Mutex<HashMap<PathBuf, Node>>, path: &Path) {
    let mut request = [0u8; 64];
    while let Ok(read) = served.read(&mut request) {
        if read == 0 {
            return;
        }
        let Some(replies) = answers(nodes, path, &request[..read]) else { return };
        for reply in replies {
            if served.write_all(&reply).is_err() {
                return;
            }
        }
    }
}

fn answers(nodes: &Mutex<HashMap<PathBuf, Node>>, path: &Path, request: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut nodes = lock(nodes);
    let node = nodes.get_mut(path)?;
    node.requests.push(request.to_vec());
    Some(match (node.answers.get(request), node.unrecorded) {
        (Some(replies), _) => replies.clone(),
        (None, Unrecorded::HidppNoDevice) => vec![no_device(request)],
        (None, Unrecorded::Silent) => Vec::new(),
    })
}

fn no_device(request: &[u8]) -> Vec<u8> {
    vec![0x10, request[1], 0x8F, request[2], request[3], NO_DEVICE, 0]
}
