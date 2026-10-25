//! Old X11 tray icons (feature `x11-tray`): runs Plasma's xembedsniproxy,
//! which puts each XEmbed icon (Wine, Java and Tk apps, Steam's fallback)
//! up as a StatusNotifierItem for the tray, and runs it again when it
//! exits, waiting longer each time it keeps failing. Nothing is started
//! when another copy already runs (a user service of the desktop's own),
//! since two would fight over the X11 tray.
//!
//! Its items all come from its own process, so the tray names them by
//! their own ids and drops one whose app has a real item too (`tray`).
//! On niri its 1x1 container window is hidden by a window rule in
//! `packaging/compositor/niri.kdl`.

use crate::logger::Logger;
use crate::{lock, tray};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const FEATURE_ID: &str = "x11-tray";
const LOG_SOURCE: &str = "x11-tray";
const PROGRAM: &str = "xembedsniproxy";
/// The name it owns on the session bus while it runs.
const BUS_NAME: &str = "org.kde.xembedsniproxy";
/// Waits before starting it again: doubling from the first while it keeps
/// failing, back to the first once it ran this long.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);
const RAN_FINE: Duration = Duration::from_secs(30);

/// Runs xembedsniproxy until stopped.
pub struct X11Tray {
    shared: Arc<Shared>,
}

struct Shared {
    /// Set by `stop`, with the condvar waking a wait before a restart.
    stopped: Mutex<bool>,
    wake: Condvar,
    /// The running copy's pid, for `stop` to end it.
    child: Mutex<Option<u32>>,
}

impl X11Tray {
    /// Starts it with `path_var` as its PATH, unless `bus` shows another copy.
    pub fn start(bus: tray::Bus, path_var: &str, logger: Arc<Logger>) -> Arc<Self> {
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

    /// Ends the running copy and doesn't start another.
    pub fn stop(&self) {
        *lock(&self.shared.stopped) = true;
        self.shared.wake.notify_all();
        if let Some(pid) = *lock(&self.shared.child) {
            // SAFETY: kill only sends a signal.
            unsafe { libc::kill(pid.cast_signed(), libc::SIGTERM) };
        }
    }
}

impl Shared {
    fn run(&self, path_var: &str, logger: &Logger) {
        let mut retry = FIRST_RETRY;
        loop {
            let started = Instant::now();
            let spawned = {
                // Locked while spawning, so `stop` either sees the pid or
                // stops the loop before the spawn.
                let mut child = lock(&self.child);
                if *lock(&self.stopped) {
                    return;
                }
                Command::new(PROGRAM)
                    .env("PATH", path_var)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                    .inspect(|spawned| *child = Some(spawned.id()))
            };
            let outcome = match spawned {
                Ok(mut spawned) => {
                    logger.info(LOG_SOURCE, &format!("started {PROGRAM} (pid {})", spawned.id()));
                    let status = spawned.wait();
                    *lock(&self.child) = None;
                    status.map_or_else(|e| e.to_string(), |status| status.to_string())
                }
                Err(e) => format!("can't start it: {e}"),
            };
            if started.elapsed() >= RAN_FINE {
                retry = FIRST_RETRY;
            }
            let stopped = lock(&self.stopped);
            if *stopped {
                return;
            }
            logger.warn(LOG_SOURCE, &format!("{PROGRAM} ended ({outcome}), starting it again in {}s", retry.as_secs()));
            let (stopped, _) =
                self.wake.wait_timeout_while(stopped, retry, |stopped| !*stopped).unwrap_or_else(|e| e.into_inner());
            if *stopped {
                return;
            }
            retry = (retry * 2).min(MAX_RETRY);
        }
    }
}

/// Whether some program owns xembedsniproxy's bus name.
fn already_running(bus: tray::Bus) -> zbus::Result<bool> {
    zbus::block_on(async {
        let builder = match bus {
            tray::Bus::Session => zbus::connection::Builder::session(),
            #[cfg(test)]
            tray::Bus::Address(address) => zbus::connection::Builder::address(address.as_str()),
        };
        let connection = builder.build().await?;
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
    use crate::hub::Hub;
    use crate::tray::tests::TestBus;
    use nova_protocol::LogLevel;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    struct Fixture {
        dir: PathBuf,
        bus: TestBus,
        logger: Arc<Logger>,
    }

    impl Fixture {
        /// A stand-in xembedsniproxy in `<dir>/bin` that writes its pid to
        /// `<dir>/runs` each time it starts, then sleeps.
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("novad-x11-tray-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            let program = dir.join("bin").join(PROGRAM);
            let script = format!("#!/bin/sh\necho $$ >> '{}'\nexec sleep 30\n", dir.join("runs").display());
            std::fs::write(&program, script).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            let bus = TestBus::start(&dir.join("bus"));
            let logger =
                Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap());
            Self { dir, bus, logger }
        }

        fn start(&self) -> Arc<X11Tray> {
            let path = format!("{}:/run/current-system/sw/bin:/usr/bin:/bin", self.dir.join("bin").display());
            X11Tray::start(tray::Bus::Address(self.bus.address.clone()), &path, Arc::clone(&self.logger))
        }

        /// The pids it started, once there are `count`.
        fn runs(&self, count: usize) -> Vec<i32> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let runs: Vec<i32> = std::fs::read_to_string(self.dir.join("runs"))
                    .unwrap_or_default()
                    .lines()
                    .map(|line| line.parse().unwrap())
                    .collect();
                if runs.len() >= count {
                    return runs;
                }
                assert!(Instant::now() < deadline, "it started {count} times");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
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
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(second) {
            assert!(Instant::now() < deadline, "stopping ends it");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(FIRST_RETRY + Duration::from_millis(200));
        assert_eq!(fixture.runs(2).len(), 2, "nothing starts after stop");
    }

    #[test]
    fn a_copy_that_already_runs_is_left_alone() {
        let fixture = Fixture::new("other");
        let other = fixture.bus.connect();
        zbus::block_on(other.request_name(BUS_NAME)).unwrap();
        let x11 = fixture.start();
        std::thread::sleep(Duration::from_millis(500));
        assert!(!fixture.dir.join("runs").exists(), "the other copy keeps the X11 tray");
        x11.stop();
    }
}
