//! Runs the Quickshell UI as a child of novad and restarts it when it exits.
//!
//! The UI's stdout and stderr go into novad's log with source `ui`. When it
//! exits on its own, the lines before the exit are saved as a crash snapshot
//! and it starts again after a backoff. When the compositor is gone, the
//! session is over and novad exits instead.

use crate::backoff::Backoff;
use crate::logger::Logger;
use crate::{child, lock};
use nova_protocol::{Crash, UiState, UiStatus};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The UI's process name in ps and btop: its `argv[0]`, and (through the
/// launcher) the name of the file the kernel runs.
pub const UI_PROCESS_NAME: &str = "NovaUI";
/// The launcher that runs Quickshell as [`UI_PROCESS_NAME`]
/// (packaging/nix/nova-ui.nix).
const UI_LAUNCHER: &str = "nova-ui";
/// The launcher's absolute path, when a package built novad with it
/// (`NOVA_UI_LAUNCHER` at build time). Then novad finds its UI however it was
/// started, without relying on the caller's `PATH`.
const PACKAGED_UI_LAUNCHER: Option<&str> = option_env!("NOVA_UI_LAUNCHER");
/// Quickshell itself, when no `nova-ui` launcher is installed.
const QUICKSHELL: &str = "quickshell";
/// Qt's Wayland plugin marks a window unexposed when a frame callback is
/// this many ms late (default 100), and redraws all of it when the late
/// callback comes. Compositors send callbacks to a covered surface only now
/// and then (niri: once a second, e.g. under a fullscreen window), so a
/// covered bar redrew itself every time one came. 0 turns the check off;
/// a covered window just waits for its next callback.
const FRAME_CALLBACK_TIMEOUT: (&str, &str) = ("QT_WAYLAND_FRAME_CALLBACK_TIMEOUT", "0");
/// Qt ties animations to vblank only while exactly one window shows; Nova
/// always has more (a stage and an exclusion zone per screen), so Qt falls
/// back to a 6 ms timer that steps 1000/refresh ms each tick: animations ran
/// 1.16x fast on 144 Hz and their frames missed vblanks. This driver steps
/// by wall-clock time on each window's own frames instead. It makes every
/// window draw while anything animates, which the stage limits to the
/// screens where something moves (ui/stage/StageMotion.qml).
const SIMPLE_ANIMATION_DRIVER: (&str, &str) = ("QSG_USE_SIMPLE_ANIMATION_DRIVER", "1");
/// With a port number in it, a debug build of novad starts the UI with a
/// QML debugger listening there, for qmlprofiler. Never
/// in a release build: anything local could connect and run code in the UI.
const QML_DEBUG_PORT_VAR: &str = "NOVA_UI_QML_DEBUG_PORT";
const LOG_SOURCE: &str = "supervisor";
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);
/// A UI that ran this long counts as healthy: the backoff starts over.
const HEALTHY_AFTER: Duration = Duration::from_secs(60);
/// A UI that dies this quickly most likely failed to load.
const EARLY_EXIT: Duration = Duration::from_secs(5);
/// How long `quickshell --version` may take; novad's start waits for it.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// The only EGL vendor file the UI loads, when there's one to pick
    /// (`egl::nvidia_only_vendor`).
    pub egl_vendor: Option<PathBuf>,
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
        if let Some(vendor) = &settings.egl_vendor {
            logger.info(LOG_SOURCE, &format!("the UI loads only NVIDIA's EGL ({})", vendor.display()));
        }
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
            pss_kib: record.pid.and_then(|pid| crate::procfs::pss_kib(Path::new(crate::procfs::PROC), pid)),
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
        // Signalled under the record lock: `run_once` clears the pid under
        // it before reaping, so the pid is still the UI's (at worst a
        // zombie) and can't have been reused by another process.
        let record = lock(&self.record);
        if let Some(pid) = record.pid
            && let Ok(pid) = libc::pid_t::try_from(pid)
        {
            // SAFETY: kill has no memory effects.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    fn run(&self, settings: &UiSettings, logger: &Arc<Logger>) {
        let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
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
                retry.reset();
            }
            let wait = retry.next();
            logger.info(LOG_SOURCE, &format!("starting the UI again in {} s", wait.as_secs()));
            std::thread::sleep(wait);
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
        // The pid is forgotten between the exit and the reap, so `stop` and
        // `status` never act on a pid the kernel may have handed out again.
        if let Err(e) = child::wait_for_exit(child.id()) {
            logger.warn(LOG_SOURCE, &format!("couldn't wait for the UI without reaping it: {e}"));
        }
        lock(&self.record).pid = None;
        let status = child.wait();
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

/// The QML debugger port [`QML_DEBUG_PORT_VAR`] asks for, in a debug build.
fn qml_debug_port(env: impl Fn(&str) -> Option<String>) -> Option<u16> {
    if !cfg!(debug_assertions) {
        return None;
    }
    env(QML_DEBUG_PORT_VAR)?.trim().parse().ok()
}

/// The packaged `nova-ui` launcher, else `nova-ui` or a `NovaUI` link to
/// Quickshell (`nova setup` makes one in `~/.local/bin`) if one is on
/// `path_var`, else `quickshell`. Either way the command line says `NovaUI`;
/// only the launcher and the link also give the process that name (btop's
/// Program column).
pub fn default_quickshell(path_var: &str) -> PathBuf {
    let on_path =
        || [UI_LAUNCHER, UI_PROCESS_NAME].into_iter().find_map(|name| nova_paths::find_program(name, path_var));
    PACKAGED_UI_LAUNCHER.map(PathBuf::from).or_else(on_path).unwrap_or_else(|| PathBuf::from(QUICKSHELL))
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
        .env(FRAME_CALLBACK_TIMEOUT.0, FRAME_CALLBACK_TIMEOUT.1)
        .env(SIMPLE_ANIMATION_DRIVER.0, SIMPLE_ANIMATION_DRIVER.1)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(port) = qml_debug_port(|name| std::env::var(name).ok()) {
        command.arg("--debug").arg(port.to_string());
    }
    if let Some(vendor) = &settings.egl_vendor {
        command.env(crate::egl::VENDOR_FILES_VAR, vendor);
    }
    // A crashed novad never leaves an orphaned bar behind. This runs on the
    // supervisor thread, which lives as long as novad.
    child::die_with_novad(&mut command, libc::SIGTERM);
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

fn quickshell_version(settings: &UiSettings, logger: &Logger) -> Option<String> {
    match read_quickshell_version(&settings.quickshell, &settings.path_var) {
        Ok(version) => {
            logger.info(LOG_SOURCE, &format!("Quickshell {version}"));
            if let Some(problem) = nova_quickshell::Range::supported().problem(&version) {
                logger.warn(LOG_SOURCE, &problem);
            }
            Some(version)
        }
        Err(problem) => {
            logger.warn(LOG_SOURCE, &problem);
            None
        }
    }
}

/// `Quickshell 0.3.2 (revision …)` → `0.3.2`, from `quickshell --version`
/// run with `path_var` as its `PATH`.
pub fn read_quickshell_version(quickshell: &Path, path_var: &str) -> Result<String, String> {
    let mut command = Command::new(quickshell);
    command.arg("--version").env("PATH", path_var);
    let stdout = stdout_within(command, VERSION_TIMEOUT)
        .map_err(|e| format!("couldn't run {} --version: {e}", quickshell.display()))?;
    String::from_utf8_lossy(&stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
        .ok_or_else(|| format!("{} --version gave no version", quickshell.display()))
}

/// What `command` prints when it succeeds within `timeout`. One that takes
/// longer, printing or exiting, is killed and reaped.
pub(crate) fn stdout_within(command: Command, timeout: Duration) -> std::io::Result<Vec<u8>> {
    let output = child::output_within(command, timeout)?;
    match output.status.success() {
        true => Ok(output.stdout),
        false => Err(std::io::Error::other(describe_exit(output.status))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_debug_build_opens_the_qml_debug_port_asked_for() {
        let env = |value: &'static str| move |name: &str| (name == QML_DEBUG_PORT_VAR).then(|| value.to_string());
        assert_eq!(qml_debug_port(env("3768")), cfg!(debug_assertions).then_some(3768));
        assert_eq!(qml_debug_port(env("not a port")), None);
        assert_eq!(qml_debug_port(|_: &str| None), None);
    }

    #[test]
    fn a_command_that_hangs_is_killed() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();
        let error = stdout_within(command, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_command_that_closes_its_output_but_hangs_is_killed() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec >&-; sleep 30"]);
        let started = Instant::now();
        let error = stdout_within(command, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// `nova setup`'s `NovaUI` link is found, after a `nova-ui` launcher.
    #[test]
    fn a_novaui_link_on_path_is_the_ui() {
        if PACKAGED_UI_LAUNCHER.is_some() {
            return; // The packaged launcher always wins.
        }
        let dir = nova_test_support::TempDir::new("ui-link");
        let path_var = dir.path_var();
        assert_eq!(default_quickshell(&path_var), PathBuf::from(QUICKSHELL));
        nova_test_support::write_program(dir.path(), UI_PROCESS_NAME, "exit 0");
        assert_eq!(default_quickshell(&path_var), dir.join(UI_PROCESS_NAME));
        nova_test_support::write_program(dir.path(), UI_LAUNCHER, "exit 0");
        assert_eq!(default_quickshell(&path_var), dir.join(UI_LAUNCHER));
    }

    #[test]
    fn a_quick_command_gives_its_output() {
        let mut command = Command::new("echo");
        command.arg("Quickshell 0.3.2");
        assert_eq!(stdout_within(command, VERSION_TIMEOUT).unwrap(), b"Quickshell 0.3.2\n");
    }
}
