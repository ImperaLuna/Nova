//! What novad's services on the session bus share: which bus to connect
//! to (the session's, or the system's for the few that read system
//! services), how they claim their names, and a stop flag that ends a
//! service's loop on the bus.

use crate::lock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::{Connection, connection};

/// Which bus a service is on.
#[derive(Clone)]
pub enum Bus {
    /// The session's, from `DBUS_SESSION_BUS_ADDRESS`.
    Session,
    /// The system's, for system services like UPower.
    System,
    /// A bus at this address, for tests.
    #[cfg(test)]
    Address(String),
}

impl Bus {
    pub fn builder(&self) -> connection::Builder<'static> {
        match self {
            Bus::Session => connection::Builder::session(),
            Bus::System => connection::Builder::system(),
            #[cfg(test)]
            Bus::Address(address) => connection::Builder::address(address.as_str()),
        }
    }
}

/// Asks the bus for `name`, queued behind an owner it already has (mako,
/// another shell's tray), which novad never replaces. Nothing replaces novad
/// either: zbus's default flags would let any other process take the name
/// (a second novad, a test's), and novad, not queued, would never get it
/// back.
pub async fn claim(connection: &Connection, name: &'static str) -> zbus::Result<RequestNameReply> {
    connection.request_name_with_flags(name, RequestNameFlags::empty()).await
}

/// Ends services' loops: set once, and wakes every loop waiting on it.
#[derive(Default)]
pub struct Stop {
    stopped: AtomicBool,
    /// Every task waiting for it: one stop can end several services.
    wakers: Mutex<Vec<Waker>>,
}

impl Stop {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        for waker in std::mem::take(&mut *lock(&self.wakers)) {
            waker.wake();
        }
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Ready once stopped; otherwise `cx` is woken by `stop`.
    pub fn poll(&self, cx: &Context<'_>) -> Poll<()> {
        // Stored before the check, so a stop in between still wakes us.
        let mut wakers = lock(&self.wakers);
        if !wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
            wakers.push(cx.waker().clone());
        }
        drop(wakers);
        if self.stopped() { Poll::Ready(()) } else { Poll::Pending }
    }
}

#[cfg(test)]
pub use test_bus::TestBus;

#[cfg(test)]
mod test_bus {
    use std::io::{BufRead, BufReader};
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use zbus::{Connection, connection};

    /// A private session bus, listening at the socket `path`.
    pub struct TestBus {
        bus: Child,
        pub address: String,
    }

    impl TestBus {
        pub fn start(path: &Path) -> Self {
            let config = path.with_extension("conf");
            std::fs::write(
                &config,
                format!(
                    "<busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth>\
                     <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                     <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
                    path.display()
                ),
            )
            .unwrap();
            let mut bus = Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--nopidfile", "--print-address=1"])
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon runs");
            let mut address = String::new();
            BufReader::new(bus.stdout.take().unwrap()).read_line(&mut address).unwrap();
            Self { bus, address: address.trim().to_string() }
        }

        /// A new connection, an app's, that answers calls to objects it serves
        /// from the moment it serves them.
        pub fn connect(&self) -> Connection {
            // `Connection::object_server` alone starts serving on a task that
            // listens only once it first runs, and a call that comes before
            // is never answered: an app that serves an item and registers it
            // at once would miss the tray reading it (IMP-161). The builder
            // returns once its object server listens.
            let builder = connection::Builder::address(self.address.as_str()).serve_at(SERVING_PATH, Serving);
            zbus::block_on(builder.build()).unwrap()
        }
    }

    /// Where a test connection serves its first object, so its object server starts with it.
    const SERVING_PATH: &str = "/org/nova/Test";

    struct Serving;

    #[zbus::interface(name = "org.nova.Test")]
    impl Serving {}

    impl Drop for TestBus {
        fn drop(&mut self) {
            let _ = self.bus.kill();
            let _ = self.bus.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Stop;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn one_stop_ends_every_loop_waiting_on_it() {
        let stop = Arc::new(Stop::default());
        let (ended, ends) = mpsc::channel();
        for _ in 0..2 {
            let (stop, ended) = (Arc::clone(&stop), ended.clone());
            std::thread::spawn(move || {
                zbus::block_on(std::future::poll_fn(|cx| stop.poll(cx)));
                let _ = ended.send(());
            });
        }
        std::thread::sleep(Duration::from_millis(100));
        stop.stop();
        for _ in 0..2 {
            ends.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
}
