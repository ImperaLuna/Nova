//! Runs the UI as a child of novad and restarts it when it exits. The UI is
//! one of two programs ([`Program`]): the QML shell under Quickshell, or
//! `nova-ui`, the GPUI program, when `features.toml` picks it
//! ([`Program::pick`]). Both get the same treatment below.
//!
//! It runs only while some feature with a UI is on ([`Supervisor::want`]):
//! with every such feature off, novad keeps serving `nova` commands without
//! a UI, and turning one on starts it again.
//!
//! The UI's stdout and stderr go into novad's log with source `ui`. When it
//! exits on its own, the lines before the exit are saved as a crash snapshot
//! and it starts again after a backoff. When the compositor is gone, the
//! session is over and novad exits instead.

use nova_base::backoff::Backoff;
use nova_base::lock;
use nova_carrier::log::UI_LOG_SOURCE;
use nova_daemon_info::{Crash, UiProgram, UiState, UiStatus};
use nova_log::Logger;
use nova_process::child::{describe_exit, stdout_within};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The UI's process name in ps and btop: its `argv[0]`, and (through the
/// launcher) the name of the file the kernel runs.
pub const UI_PROCESS_NAME: &str = "NovaUI";
/// The launcher that runs Quickshell as [`UI_PROCESS_NAME`]
/// (packaging/nix/nova-quickshell.nix).
const QUICKSHELL_LAUNCHER: &str = "nova-quickshell";
/// The launcher's absolute path, when a package built novad with it
/// (`NOVA_QUICKSHELL_LAUNCHER` at build time). Then novad finds its UI
/// however it was started, without relying on the caller's `PATH`.
const PACKAGED_QUICKSHELL_LAUNCHER: Option<&str> = option_env!("NOVA_QUICKSHELL_LAUNCHER");
/// Quickshell itself, when no `nova-quickshell` launcher is installed.
const QUICKSHELL: &str = "quickshell";
/// Tells the UI where novad listens.
const SOCKET_VAR: &str = "NOVA_SOCKET";
/// The GPUI program, looked up on the session's `PATH`.
const GPUI_UI: &str = "nova-ui";
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
const NOT_NEEDED: &str = "no feature with a UI is on, so the UI isn't running";
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);
/// A UI that ran this long counts as healthy: the backoff starts over.
const HEALTHY_AFTER: Duration = Duration::from_secs(60);
/// A UI that dies this quickly most likely failed to load.
const EARLY_EXIT: Duration = Duration::from_secs(5);
/// How often novad checks the compositor is still there while no UI runs
/// (a UI's exit is the check otherwise).
const SESSION_CHECK: Duration = Duration::from_secs(5);
/// How long `quickshell --version` may take; novad's start waits for it.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// The program novad runs as the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// Quickshell ([`UiSettings::quickshell`]) on the QML shell
    /// ([`UiSettings::ui_dir`]).
    Quickshell,
    /// `nova-ui`, the GPUI program, at this path.
    Gpui(PathBuf),
}

impl Program {
    /// The program `picked` names (`features.toml`'s `ui`). The GPUI
    /// program is `gpui_ui` when novad was given one, else `nova-ui` on
    /// `path_var`. When it isn't there, Quickshell runs instead and the
    /// second value says why, so a setting never leaves the session without
    /// a shell.
    pub fn pick(picked: UiProgram, gpui_ui: Option<&Path>, path_var: &str) -> (Self, Option<String>) {
        if picked == UiProgram::Quickshell {
            return (Self::Quickshell, None);
        }
        let found = match gpui_ui {
            Some(path) => nova_paths::is_executable(path).then(|| path.to_path_buf()),
            None => nova_paths::find_program(GPUI_UI, path_var),
        };
        match (found, gpui_ui) {
            (Some(path), _) => (Self::Gpui(path), None),
            (None, Some(path)) => (Self::Quickshell, Some(format!("{} isn't a program", path.display()))),
            (None, None) => (Self::Quickshell, Some(format!("{GPUI_UI} isn't on PATH"))),
        }
    }

    const fn kind(&self) -> UiProgram {
        match self {
            Self::Quickshell => UiProgram::Quickshell,
            Self::Gpui(_) => UiProgram::Gpui,
        }
    }
}

#[derive(Clone)]
pub struct UiSettings {
    /// Which program runs.
    pub program: Program,
    /// The Quickshell binary.
    pub quickshell: PathBuf,
    /// The UI's QML folder, the one holding `shell.qml`.
    pub ui_dir: PathBuf,
    /// Passed to the UI as [`SOCKET_VAR`].
    pub socket: PathBuf,
    /// `PATH` for the UI and its children.
    pub path_var: String,
    /// The compositor's Wayland socket. When it's gone after the UI exits,
    /// the session is over and novad exits.
    pub wayland_socket: Option<PathBuf>,
    /// The only EGL vendor file the UI loads, when there's one to pick
    /// (`egl::nvidia_only_vendor`).
    pub egl_vendor: Option<PathBuf>,
    /// The Qt render loop the UI uses, unless the session picks one
    /// (`render_loop::for_ui`).
    pub render_loop: Option<&'static str>,
    /// The icon theme novad finds icons in, passed as `QS_ICON_THEME` so
    /// what Qt still looks up itself (tray menus) matches.
    pub icon_theme: String,
}

impl UiSettings {
    /// The file novad runs as the UI.
    fn program_path(&self) -> &Path {
        match &self.program {
            Program::Quickshell => &self.quickshell,
            Program::Gpui(program) => program,
        }
    }
}

pub struct Supervisor {
    record: Mutex<Record>,
    /// Signalled when `wanted` or `quitting` changes.
    changed: Condvar,
    quitting: AtomicBool,
    program: UiProgram,
    quickshell_version: Option<String>,
}

#[derive(Default)]
struct Record {
    /// Some feature with a UI is on, so the UI should run.
    wanted: bool,
    /// The running UI was told to stop because it's no longer wanted, so
    /// its exit isn't a crash even if it's wanted again by then.
    stopping: bool,
    pid: Option<u32>,
    restarts: u32,
    last_crash: Option<Crash>,
}

impl Supervisor {
    /// Starts the UI on a thread of its own that lives as long as novad,
    /// running it while `wanted` (see [`Self::want`]).
    pub fn start(settings: UiSettings, wanted: bool, logger: Arc<Logger>) -> Arc<Self> {
        let quickshell_version = match &settings.program {
            Program::Quickshell => announce_quickshell(&settings, &logger),
            Program::Gpui(program) => {
                logger.info(LOG_SOURCE, &format!("the UI is the GPUI program, {}", program.display()));
                None
            }
        };
        if !wanted {
            logger.info(LOG_SOURCE, NOT_NEEDED);
        }
        let supervisor = Arc::new(Self {
            record: Mutex::new(Record { wanted, ..Record::default() }),
            changed: Condvar::new(),
            quitting: AtomicBool::new(false),
            program: settings.program.kind(),
            quickshell_version,
        });
        let worker = Arc::clone(&supervisor);
        std::thread::spawn(move || worker.run(&settings, &logger));
        supervisor
    }

    pub fn status(&self) -> UiStatus {
        let record = lock(&self.record);
        let state = match (record.pid, record.wanted) {
            (Some(_), _) => UiState::Running,
            (None, true) => UiState::Restarting,
            (None, false) => UiState::NotNeeded,
        };
        UiStatus {
            program: self.program,
            state,
            pid: record.pid,
            pss_kib: record
                .pid
                .and_then(|pid| nova_process::procfs::pss_kib(Path::new(nova_process::procfs::PROC), pid)),
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
        let record = lock(&self.record);
        terminate(&record);
        self.changed.notify_all();
    }

    /// Runs the UI while `wanted` (some feature with a UI is on): stops it
    /// when that turns false and starts it when it turns true again.
    pub fn want(&self, wanted: bool, logger: &Logger) {
        let mut record = lock(&self.record);
        if record.wanted == wanted {
            return;
        }
        record.wanted = wanted;
        if wanted {
            logger.info(LOG_SOURCE, "a feature with a UI is on: starting the UI");
        } else {
            logger.info(LOG_SOURCE, NOT_NEEDED);
            record.stopping = record.pid.is_some();
            terminate(&record);
        }
        self.changed.notify_all();
    }

    fn run(&self, settings: &UiSettings, logger: &Arc<Logger>) {
        let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
        loop {
            loop {
                let record = lock(&self.record);
                let (record, _timeout) = self
                    .changed
                    .wait_timeout_while(record, SESSION_CHECK, |record| {
                        !record.wanted && !self.quitting.load(Ordering::SeqCst)
                    })
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if record.wanted || self.quitting.load(Ordering::SeqCst) {
                    break;
                }
                drop(record);
                exit_if_session_over(settings, logger);
            }
            if self.quitting.load(Ordering::SeqCst) {
                return;
            }
            let started = Instant::now();
            let exit = self.run_once(settings, logger);
            if self.quitting.load(Ordering::SeqCst) {
                return;
            }
            if std::mem::take(&mut lock(&self.record).stopping) {
                // Stopped on purpose: no crash, and a fresh backoff.
                retry.reset();
                continue;
            }
            exit_if_session_over(settings, logger);
            let ran_for = started.elapsed();
            self.record_crash(&exit, ran_for, logger);
            if ran_for > HEALTHY_AFTER {
                retry.reset();
            }
            let wait = retry.next();
            logger.info(LOG_SOURCE, &format!("starting the UI again in {} s", wait.as_secs()));
            // Cut short by `stop`, or by `want(false)` (the loop then waits
            // until it's wanted again).
            let record = lock(&self.record);
            let _waited = self
                .changed
                .wait_timeout_while(record, wait, |record| record.wanted && !self.quitting.load(Ordering::SeqCst))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Runs the UI until it exits. Returns how it ended, as text for the log.
    fn run_once(&self, settings: &UiSettings, logger: &Arc<Logger>) -> String {
        // Spawned under the record lock, so a `want(false)` or `stop` either
        // comes first (and nothing starts) or sees the pid and ends it.
        let mut child = {
            let mut record = lock(&self.record);
            if !record.wanted || self.quitting.load(Ordering::SeqCst) {
                return "not started: not wanted".into();
            }
            match spawn(settings) {
                Ok(child) => {
                    record.pid = Some(child.id());
                    child
                }
                Err(e) => return format!("couldn't start {}: {e}", settings.program_path().display()),
            }
        };
        let source = match &settings.program {
            Program::Quickshell => &settings.ui_dir,
            Program::Gpui(program) => program,
        };
        logger.info(LOG_SOURCE, &format!("UI started (pid {}) from {}", child.id(), source.display()));
        let readers = [
            child.stdout.take().map(|out| forward_lines(out, Arc::clone(logger), UI_LOG_SOURCE)),
            child.stderr.take().map(|err| forward_lines(err, Arc::clone(logger), UI_LOG_SOURCE)),
        ];
        // The pid is forgotten between the exit and the reap, so `stop` and
        // `status` never act on a pid the kernel may have handed out again.
        if let Err(e) = nova_process::child::wait_for_exit(child.id()) {
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
        record.last_crash = Some(Crash { time: nova_log::now(), log: snapshot });
    }
}

/// Sends the UI SIGTERM, if it runs. Called under the record lock:
/// `run_once` clears the pid under it before reaping, so the pid is still
/// the UI's (at worst a zombie) and can't have been reused by another
/// process.
/// Exits novad once the compositor's socket is gone: the session is over.
fn exit_if_session_over(settings: &UiSettings, logger: &Logger) {
    if settings.wayland_socket.as_ref().is_some_and(|socket| !socket.exists()) {
        logger.info(LOG_SOURCE, "the compositor is gone, so the session is over: exiting");
        std::process::exit(0);
    }
}

fn terminate(record: &Record) {
    if let Some(pid) = record.pid
        && let Ok(pid) = libc::pid_t::try_from(pid)
    {
        // SAFETY: kill has no memory effects.
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
}

/// The QML debugger port [`QML_DEBUG_PORT_VAR`] asks for, in a debug build.
fn qml_debug_port(env: impl Fn(&str) -> Option<String>) -> Option<u16> {
    if !cfg!(debug_assertions) {
        return None;
    }
    env(QML_DEBUG_PORT_VAR)?.trim().parse().ok()
}

/// The packaged `nova-quickshell` launcher, else `nova-quickshell` or a `NovaUI` link to
/// Quickshell (`nova setup` makes one in `~/.local/bin`) if one is on
/// `path_var`, else `quickshell`. Either way the command line says `NovaUI`;
/// only the launcher and the link also give the process that name (btop's
/// Program column).
pub fn default_quickshell(path_var: &str) -> PathBuf {
    let on_path =
        || [QUICKSHELL_LAUNCHER, UI_PROCESS_NAME].into_iter().find_map(|name| nova_paths::find_program(name, path_var));
    PACKAGED_QUICKSHELL_LAUNCHER.map(PathBuf::from).or_else(on_path).unwrap_or_else(|| PathBuf::from(QUICKSHELL))
}

/// Quickshell running the shell at `path` (a folder holding `shell.qml`, or
/// a `.qml` file whose folder is the root its imports resolve in) as
/// `process_name`, connected to novad, with the session's settings. The
/// caller picks its input and output, and ties it to novad's life.
pub fn quickshell_command(settings: &UiSettings, path: &Path, process_name: &str) -> Command {
    let mut command = Command::new(&settings.quickshell);
    command
        .arg0(process_name)
        // Plain lines: novad reads the level from them and logs them itself.
        .arg("--no-color")
        .arg("--path")
        .arg(path)
        .env(SOCKET_VAR, &settings.socket)
        .env("PATH", &settings.path_var)
        .env(FRAME_CALLBACK_TIMEOUT.0, FRAME_CALLBACK_TIMEOUT.1)
        .env(SIMPLE_ANIMATION_DRIVER.0, SIMPLE_ANIMATION_DRIVER.1)
        .env(nova_icons::QUICKSHELL_VAR, &settings.icon_theme);
    if let Some(vendor) = &settings.egl_vendor {
        command.env(crate::egl::VENDOR_FILES_VAR, vendor);
    }
    if let Some(render_loop) = settings.render_loop {
        command.env(crate::render_loop::VAR, render_loop);
    }
    command
}

/// The GPUI program at `program` as [`UI_PROCESS_NAME`], connected to novad.
/// It draws through Vulkan and has no Qt, so none of Quickshell's settings
/// apply.
fn gpui_command(settings: &UiSettings, program: &Path) -> Command {
    let mut command = Command::new(program);
    command.arg0(UI_PROCESS_NAME).env(SOCKET_VAR, &settings.socket).env("PATH", &settings.path_var);
    command
}

fn spawn(settings: &UiSettings) -> std::io::Result<Child> {
    let mut command = match &settings.program {
        Program::Quickshell => {
            let mut command = quickshell_command(settings, &settings.ui_dir, UI_PROCESS_NAME);
            if let Some(port) = qml_debug_port(|name| std::env::var(name).ok()) {
                command.arg("--debug").arg(port.to_string());
            }
            command
        }
        Program::Gpui(program) => gpui_command(settings, program),
    };
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A crashed novad never leaves an orphaned bar behind. This runs on the
    // supervisor thread, which lives as long as novad.
    nova_process::child::die_with_novad(&mut command, libc::SIGTERM);
    command.spawn()
}

/// Logs each line a UI process writes to `stream`, as from
/// `source`, until it closes.
pub fn forward_lines(stream: impl Read + Send + 'static, logger: Arc<Logger>, source: &'static str) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
            logger.ui_line(source, &String::from_utf8_lossy(&line));
            line.clear();
        }
    })
}

/// Logs how Quickshell runs: its version (returned, with a warning outside
/// the tested range), and the EGL and render loop picked for it.
fn announce_quickshell(settings: &UiSettings, logger: &Logger) -> Option<String> {
    if let Some(vendor) = &settings.egl_vendor {
        logger.info(LOG_SOURCE, &format!("the UI loads only NVIDIA's EGL ({})", vendor.display()));
    }
    if let Some(render_loop) = settings.render_loop {
        logger.info(LOG_SOURCE, &format!("the UI uses Qt's {render_loop} render loop"));
    }
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

    /// `nova setup`'s `NovaUI` link is found, after a `nova-quickshell` launcher.
    #[test]
    fn a_novaui_link_on_path_is_the_ui() {
        if PACKAGED_QUICKSHELL_LAUNCHER.is_some() {
            return; // The packaged launcher always wins.
        }
        let dir = nova_test_support::TempDir::new("ui-link");
        let path_var = dir.path_var();
        assert_eq!(default_quickshell(&path_var), PathBuf::from(QUICKSHELL));
        nova_test_support::write_program(dir.path(), UI_PROCESS_NAME, "exit 0");
        assert_eq!(default_quickshell(&path_var), dir.join(UI_PROCESS_NAME));
        nova_test_support::write_program(dir.path(), QUICKSHELL_LAUNCHER, "exit 0");
        assert_eq!(default_quickshell(&path_var), dir.join(QUICKSHELL_LAUNCHER));
    }

    #[test]
    fn the_gpui_program_runs_only_when_picked_and_found() {
        let dir = nova_test_support::TempDir::new("gpui-pick");
        let path_var = dir.path_var();
        let pick = |picked, gpui_ui: Option<&Path>| Program::pick(picked, gpui_ui, &path_var);

        // Not there: Quickshell runs, and the caller hears why.
        let (program, problem) = pick(UiProgram::Gpui, None);
        assert_eq!((program, problem.as_deref()), (Program::Quickshell, Some("nova-ui isn't on PATH")));
        let missing = dir.join("elsewhere");
        let (program, problem) = pick(UiProgram::Gpui, Some(&missing));
        assert_eq!(program, Program::Quickshell);
        assert!(problem.is_some_and(|problem| problem.contains("elsewhere")));
        // A file that can't run is no better than a missing one.
        let plain = dir.join("plain");
        std::fs::write(&plain, "").unwrap();
        assert_eq!(pick(UiProgram::Gpui, Some(&plain)).0, Program::Quickshell);

        let on_path = nova_test_support::write_program(dir.path(), GPUI_UI, "exit 0");
        assert_eq!(pick(UiProgram::Gpui, None), (Program::Gpui(on_path.clone()), None));
        // The one novad was given wins over PATH's.
        let given = nova_test_support::write_program(dir.path(), "given", "exit 0");
        assert_eq!(pick(UiProgram::Gpui, Some(&given)), (Program::Gpui(given), None));
        // Installed but not picked: nothing changes.
        assert_eq!(pick(UiProgram::Quickshell, Some(&on_path)), (Program::Quickshell, None));
    }

    #[test]
    fn a_quick_command_gives_its_output() {
        let mut command = Command::new("echo");
        command.arg("Quickshell 0.3.2");
        assert_eq!(stdout_within(command, VERSION_TIMEOUT).unwrap(), b"Quickshell 0.3.2\n");
    }
}
