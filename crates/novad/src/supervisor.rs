//! Runs the Quickshell UI as a child of novad and restarts it when it exits.
//!
//! The UI's stdout and stderr go into novad's log with source `ui`. When it
//! exits on its own, the lines before the exit are saved as a crash snapshot
//! and it starts again after a backoff. When the compositor is gone, the
//! session is over and novad exits instead.

use crate::lock;
use crate::logger::Logger;
use nova_protocol::{Crash, UiState, UiStatus};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The UI's process name in ps and btop: its `argv[0]`, and the name of the
/// launcher that runs Quickshell under it, so the kernel's name matches.
pub const UI_PROCESS_NAME: &str = "nova-ui";
/// Quickshell itself, when no `nova-ui` launcher is installed.
const QUICKSHELL: &str = "quickshell";
const LOG_SOURCE: &str = "supervisor";
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);
/// A UI that ran this long counts as healthy: the backoff starts over.
const HEALTHY_AFTER: Duration = Duration::from_secs(60);
/// A UI that dies this quickly most likely failed to load.
const EARLY_EXIT: Duration = Duration::from_secs(5);

pub struct UiSettings {
    /// The Quickshell binary.
    pub quickshell: PathBuf,
    /// The UI's QML folder, the one holding `shell.qml`.
    pub ui_dir: PathBuf,
    /// Passed to the UI as `NOVA_SOCKET`.
    pub socket: PathBuf,
    /// `PATH` for the UI and its children.
    pub path_var: String,
    /// The compositor's Wayland socket. When it's gone after the UI exits,
    /// the session is over and novad exits.
    pub wayland_socket: Option<PathBuf>,
}

pub struct Supervisor {
    record: Mutex<Record>,
    quitting: AtomicBool,
    quickshell_version: Option<String>,
}

#[derive(Default)]
struct Record {
    pid: Option<u32>,
    restarts: u32,
    last_crash: Option<Crash>,
}

impl Supervisor {
    /// Starts the UI on a thread of its own that lives as long as novad.
    pub fn start(settings: UiSettings, logger: Arc<Logger>) -> Arc<Self> {
        let quickshell_version = quickshell_version(&settings, &logger);
        let supervisor =
            Arc::new(Self { record: Mutex::default(), quitting: AtomicBool::new(false), quickshell_version });
        let worker = Arc::clone(&supervisor);
        std::thread::spawn(move || worker.run(&settings, &logger));
        supervisor
    }

    pub fn status(&self) -> UiStatus {
        let record = lock(&self.record);
        UiStatus {
            state: if record.pid.is_some() { UiState::Running } else { UiState::Restarting },
            pid: record.pid,
            pss_kib: record.pid.and_then(crate::procfs::pss_kib),
            restarts: record.restarts,
            last_crash: record.last_crash.clone(),
        }
    }

    pub fn quickshell_version(&self) -> Option<String> {
        self.quickshell_version.clone()
    }

    /// Stops the UI for good; it won't be restarted.
    pub fn stop(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        if let Some(pid) = lock(&self.record).pid
            && let Ok(pid) = libc::pid_t::try_from(pid)
        {
            // SAFETY: kill has no memory effects; a stale pid at worst
            // signals nothing, since novad hasn't reaped the child yet.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    fn run(&self, settings: &UiSettings, logger: &Arc<Logger>) {
        let mut retry = FIRST_RETRY;
        loop {
            let started = Instant::now();
            let exit = self.run_once(settings, logger);
            if self.quitting.load(Ordering::SeqCst) {
                return;
            }
            if settings.wayland_socket.as_ref().is_some_and(|socket| !socket.exists()) {
                logger.info(LOG_SOURCE, "the compositor is gone, so the session is over: exiting");
                std::process::exit(0);
            }
            let ran_for = started.elapsed();
            self.record_crash(&exit, ran_for, logger);
            if ran_for > HEALTHY_AFTER {
                retry = FIRST_RETRY;
            }
            logger.info(LOG_SOURCE, &format!("starting the UI again in {} s", retry.as_secs()));
            std::thread::sleep(retry);
            retry = (retry * 2).min(MAX_RETRY);
        }
    }

    /// Runs the UI until it exits. Returns how it ended, as text for the log.
    fn run_once(&self, settings: &UiSettings, logger: &Arc<Logger>) -> String {
        let mut child = match spawn(settings) {
            Ok(child) => child,
            Err(e) => return format!("couldn't start {}: {e}", settings.quickshell.display()),
        };
        lock(&self.record).pid = Some(child.id());
        logger.info(LOG_SOURCE, &format!("UI started (pid {}) from {}", child.id(), settings.ui_dir.display()));
        let readers = [
            child.stdout.take().map(|out| forward_lines(out, Arc::clone(logger))),
            child.stderr.take().map(|err| forward_lines(err, Arc::clone(logger))),
        ];
        let status = child.wait();
        lock(&self.record).pid = None;
        // Every line the UI printed is in the log before its exit is.
        for reader in readers.into_iter().flatten() {
            let _ = reader.join();
        }
        match status {
            Ok(status) => describe_exit(status),
            Err(e) => format!("couldn't wait for the UI: {e}"),
        }
    }

    fn record_crash(&self, exit: &str, ran_for: Duration, logger: &Logger) {
        logger.error(LOG_SOURCE, &format!("the UI stopped after {} s: {exit}", ran_for.as_secs()));
        if ran_for < EARLY_EXIT {
            logger.error(
                LOG_SOURCE,
                "it stopped right after starting, so it most likely failed to load: see the UI lines just above",
            );
        }
        let snapshot = match logger.save_crash() {
            Ok(path) => path.display().to_string(),
            Err(e) => {
                logger.error(LOG_SOURCE, &format!("couldn't save the crash log: {e}"));
                String::new()
            }
        };
        let mut record = lock(&self.record);
        record.restarts += 1;
        record.last_crash = Some(Crash { time: crate::logger::now(), log: snapshot });
    }
}

/// The `nova-ui` launcher if it's on `path_var`, else `quickshell`. Either
/// way the command line says `nova-ui`; only the launcher also gives the
/// process that name (btop's Program column).
pub fn default_quickshell(path_var: &str) -> PathBuf {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(UI_PROCESS_NAME))
        .find(|launcher| launcher.is_file())
        .unwrap_or_else(|| PathBuf::from(QUICKSHELL))
}

fn spawn(settings: &UiSettings) -> std::io::Result<Child> {
    let mut command = Command::new(&settings.quickshell);
    command
        .arg0(UI_PROCESS_NAME)
        // Plain lines: novad reads the level from them and logs them itself.
        .arg("--no-color")
        .arg("--path")
        .arg(&settings.ui_dir)
        .env("NOVA_SOCKET", &settings.socket)
        .env("PATH", &settings.path_var)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: the closure runs between fork and exec and only calls prctl,
    // which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            // The UI gets SIGTERM when novad dies, however it dies, so a
            // crashed novad never leaves an orphaned bar behind. The signal
            // follows the thread that spawned the child; this runs on the
            // supervisor thread, which lives as long as novad.
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

/// Logs each line from `stream` as the UI's until it closes.
fn forward_lines(stream: impl Read + Send + 'static, logger: Arc<Logger>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
            logger.ui_line(&String::from_utf8_lossy(&line));
            line.clear();
        }
    })
}

fn describe_exit(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => format!("killed by signal {signal}"),
        (None, None) => "unknown exit".into(),
    }
}

/// `Quickshell 0.3.2 (revision …)` → `0.3.2`.
fn quickshell_version(settings: &UiSettings, logger: &Logger) -> Option<String> {
    let output = Command::new(&settings.quickshell).arg("--version").env("PATH", &settings.path_var).output();
    let version = output
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8_lossy(&output.stdout).split_whitespace().nth(1).map(str::to_owned));
    match &version {
        Some(version) => logger.info(LOG_SOURCE, &format!("Quickshell {version}")),
        None => logger.warn(LOG_SOURCE, &format!("{} --version gave no version", settings.quickshell.display())),
    }
    version
}
