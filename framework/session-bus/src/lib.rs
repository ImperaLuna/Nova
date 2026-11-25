use nova_base::lock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::{Connection, connection};

#[derive(Clone)]
pub enum Bus {
    Session,
    System,
    #[cfg(any(test, feature = "test-support"))]
    Address(String),
}

impl Bus {
    pub fn builder(&self) -> connection::Builder<'static> {
        match self {
            Bus::Session => connection::Builder::session(),
            Bus::System => connection::Builder::system(),
            #[cfg(any(test, feature = "test-support"))]
            Bus::Address(address) => connection::Builder::address(address.as_str()),
        }
    }
}

pub async fn claim(connection: &Connection, name: &'static str) -> zbus::Result<RequestNameReply> {
    // With no flags the bus queues the request and never hands the name to another program.
    // https://dbus.freedesktop.org/doc/dbus-specification.html#bus-messages-request-name
    connection.request_name_with_flags(name, RequestNameFlags::empty()).await
}

#[derive(Default)]
pub struct Stop {
    stopped: AtomicBool,
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

    pub fn poll(&self, cx: &Context<'_>) -> Poll<()> {
        // The waker is stored before the check, so a stop in between still wakes this task: https://linear.app/imperaluna/issue/IMP-75
        let mut wakers = lock(&self.wakers);
        if !wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
            wakers.push(cx.waker().clone());
        }
        drop(wakers);
        if self.stopped() { Poll::Ready(()) } else { Poll::Pending }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use test_bus::TestBus;

#[cfg(any(test, feature = "test-support"))]
#[expect(clippy::unwrap_used, clippy::expect_used, reason = "test helpers fail the test that calls them, loudly")]
mod test_bus {
    use std::io::{BufRead, BufReader};
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use zbus::{Connection, connection};

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

        pub fn connect(&self) -> Connection {
            // `Connection::object_server` answers no call until its task first runs; the builder returns once it listens.
            // https://docs.rs/zbus/latest/zbus/connection/struct.Builder.html#method.serve_at
            let builder = connection::Builder::address(self.address.as_str()).serve_at(SERVING_PATH, Serving);
            zbus::block_on(builder.build()).unwrap()
        }
    }

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
        nova_test_support::settle(Duration::from_millis(100));
        stop.stop();
        for _ in 0..2 {
            ends.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
}
