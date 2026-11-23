use nova_base::backoff::Restarts;
use nova_base::lock;
use nova_log::Logger;
use nova_process::child;
use nova_session_bus::Bus;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, mpsc};

pub const FEATURE_ID: &str = "x11-tray";
const LOG_SOURCE: &str = "x11-tray";
const PROGRAM: &str = "xembedsniproxy";
const BUS_NAME: &str = "org.kde.xembedsniproxy";

pub struct X11Tray {
    shared: Arc<Shared>,
}

struct Shared {
    stopped: Mutex<bool>,
    wake: Condvar,
    child: Mutex<Option<u32>>,
}

impl X11Tray {
    pub fn start(bus: Bus, path_var: &str, logger: Arc<Logger>) -> Arc<Self> {
        let shared = Arc::new(Shared { stopped: Mutex::new(false), wake: Condvar::new(), child: Mutex::new(None) });
        let running = Arc::clone(&shared);
        let path_var = path_var.to_owned();
        std::thread::spawn(move || {
            match already_running(bus) {
                Ok(true) => {
                    return logger.info(LOG_SOURCE, &format!("{PROGRAM} already runs, so novad leaves it to that one"));
                }
                Ok(false) => {}
                Err(e) => logger.warn(LOG_SOURCE, &format!("can't tell whether {PROGRAM} runs, starting it: {e}")),
            }
            running.run(&path_var, &logger);
        });
        Arc::new(Self { shared })
    }

    pub fn stop(&self) {
        *lock(&self.shared.stopped) = true;
        self.shared.wake.notify_all();
        if let Some(pid) = *lock(&self.shared.child) {
            child::terminate(pid);
        }
    }
}

impl Shared {
    fn run(&self, path_var: &str, logger: &Logger) {
        let mut restarts = Restarts::default();
        loop {
            restarts.started();
            let (exited, exit) = mpsc::channel();
            let spawned = {
                let mut child = lock(&self.child);
                if *lock(&self.stopped) {
                    return;
                }
                let mut command = Command::new(PROGRAM);
                command.env("PATH", path_var);
                child::watch(&mut command, move |gone| _ = exited.send(gone)).inspect(|pid| *child = Some(*pid))
            };
            let outcome = match spawned {
                Ok(pid) => {
                    logger.info(LOG_SOURCE, &format!("started {PROGRAM} (pid {pid})"));
                    let gone = exit.recv();
                    *lock(&self.child) = None;
                    gone.map_or_else(|_| "its watcher went away".into(), child::Exited::reap)
                }
                Err(e) => format!("can't start it: {e}"),
            };
            let stopped = lock(&self.stopped);
            if *stopped {
                return;
            }
            let wait = restarts.after_exit();
            logger.warn(LOG_SOURCE, &format!("{PROGRAM} ended ({outcome}), starting it again in {}s", wait.as_secs()));
            let (stopped, _) =
                self.wake.wait_timeout_while(stopped, wait, |stopped| !*stopped).unwrap_or_else(|e| e.into_inner());
            if *stopped {
                return;
            }
        }
    }
}

fn already_running(bus: Bus) -> zbus::Result<bool> {
    zbus::block_on(async {
        let connection = bus.builder().build().await?;
        let reply = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(BUS_NAME,),
            )
            .await?;
        reply.body().deserialize::<bool>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_carrier::log::LogLevel;
    use nova_hub::Hub;
    use nova_session_bus::TestBus;
    use nova_test_support::{eventually, wait_until};
    use std::path::Path;
    use std::time::Duration;

    struct Fixture {
        dir: nova_test_support::TempDir,
        bus: TestBus,
        logger: Arc<Logger>,
        path: String,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = nova_test_support::TempDir::new(&format!("x11-tray-{name}"));
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            let script = format!("echo $$ >> '{}'\nexec sleep 30", dir.join("runs").display());
            nova_test_support::write_program(&dir.join("bin"), PROGRAM, &script);
            let bus = TestBus::start(&dir.join("bus"));
            let logger =
                Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap());
            let path = nova_test_support::fake_path(&dir.join("bin"), &["sleep"]);
            Self { dir, bus, logger, path }
        }

        fn start(&self) -> Arc<X11Tray> {
            X11Tray::start(Bus::Address(self.bus.address.clone()), &self.path, Arc::clone(&self.logger))
        }

        fn runs(&self, count: usize) -> Vec<i32> {
            eventually(Duration::from_secs(5), &format!("it to start {count} times"), || {
                let runs: Vec<i32> = std::fs::read_to_string(self.dir.join("runs"))
                    .unwrap_or_default()
                    .lines()
                    .map(|line| line.parse().unwrap())
                    .collect();
                (runs.len() >= count).then_some(runs)
            })
        }
    }

    fn alive(pid: i32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
            && !std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default().contains(") Z ")
    }

    #[test]
    fn it_runs_again_after_a_crash_and_goes_on_stop() {
        let fixture = Fixture::new("crash");
        let x11 = fixture.start();
        let first = fixture.runs(1)[0];
        // SAFETY: kill only sends a signal.
        unsafe { libc::kill(first, libc::SIGKILL) };
        let second = fixture.runs(2)[1];
        assert_ne!(first, second);
        x11.stop();
        wait_until(Duration::from_secs(5), "stopping to end it", || !alive(second));
        nova_test_support::settle(Restarts::FIRST + Duration::from_millis(200));
        assert_eq!(fixture.runs(2).len(), 2, "nothing starts after stop");
    }

    #[test]
    fn a_copy_that_already_runs_is_left_alone() {
        let fixture = Fixture::new("other");
        let other = fixture.bus.connect();
        zbus::block_on(other.request_name(BUS_NAME)).unwrap();
        let x11 = fixture.start();
        nova_test_support::settle(Duration::from_millis(500));
        assert!(!fixture.dir.join("runs").exists(), "the other copy keeps the X11 tray");
        x11.stop();
    }
}
