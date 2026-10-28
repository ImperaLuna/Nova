//! Reports to and from one hidraw node: each write sends one report, each
//! read returns one. Reads wait with `poll` on the node and on an eventfd
//! that stopping writes to, so a reader blocked on a quiet receiver ends
//! at once.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Instant;

/// The longest HID++ report, and more: one read takes one report.
const READ_BYTES: usize = 64;

/// Ends a [`Channel`]'s waits, from any thread.
#[derive(Clone)]
pub struct Stopper(Arc<std::fs::File>);

impl Stopper {
    pub fn new() -> std::io::Result<Self> {
        // SAFETY: no pointers; returns a new descriptor or -1.
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) };
        if fd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` was just opened and nothing else owns it.
        Ok(Self(Arc::new(unsafe { OwnedFd::from_raw_fd(fd) }.into())))
    }

    pub fn stop(&self) {
        // An eventfd adds the 8-byte number written to its counter.
        if let Err(e) = (&*self.0).write_all(&1u64.to_ne_bytes()) {
            eprintln!("novad: can't stop a hidraw reader: {e}");
        }
    }
}

/// What a wait for a report ended with.
pub enum Received {
    Report(Vec<u8>),
    TimedOut,
    Stopped,
}

pub struct Channel {
    node: std::fs::File,
    stop: Stopper,
}

impl Channel {
    pub fn new(node: OwnedFd, stop: Stopper) -> Self {
        Self { node: node.into(), stop }
    }

    pub fn send(&mut self, report: &[u8]) -> std::io::Result<()> {
        self.node.write_all(report)
    }

    /// The next report, waiting until `until` at most.
    pub fn receive(&mut self, until: Instant) -> std::io::Result<Received> {
        let mut fds = [
            libc::pollfd { fd: self.node.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: self.stop.0.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        loop {
            let left = until.saturating_duration_since(Instant::now());
            // Rounded up, so a wait never ends just before `until`.
            let timeout = i32::try_from(left.as_millis() + 1).unwrap_or(i32::MAX);
            // SAFETY: `fds` is a valid array of two pollfds for the call.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
            if ready == -1 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if fds[1].revents != 0 {
                return Ok(Received::Stopped);
            }
            if fds[0].revents & libc::POLLIN != 0 {
                let mut report = vec![0; READ_BYTES];
                let read = self.node.read(&mut report)?;
                if read == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                report.truncate(read);
                return Ok(Received::Report(report));
            }
            if fds[0].revents != 0 {
                // Unplugged: the node hangs up.
                return Err(std::io::Error::from_raw_os_error(libc::ENODEV));
            }
            if Instant::now() >= until {
                return Ok(Received::TimedOut);
            }
        }
    }
}
