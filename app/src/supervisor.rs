//! Runs the UI, `nova-ui` (the GPUI program), as a child of novad and
//! restarts it when it exits. A `nova-ui` that is missing or won't start
//! is an error in the log (once per reason) and another try after a
//! backoff, so installing it later brings the UI back with no restart.
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
use nova_daemon_info::{Crash, SOCKET_VAR, UiProgram, UiState, UiStatus};
use nova_log::Logger;
use nova_process::child::describe_exit;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The UI's process name in ps and btop: its `argv[0]`.
pub const UI_PROCESS_NAME: &str = "NovaUI";
/// The GPUI program, looked up on the session's `PATH` at each start.
pub const GPUI_UI: &str = "nova-ui";
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

#[derive(Clone)]
pub struct UiSettings {
    /// The program novad was told to run (`nova daemon --gpui-ui`), else
    /// [`GPUI_UI`] found on `path_var` at each start.
    pub program: Option<PathBuf>,
    /// Passed to the UI as [`SOCKET_VAR`].
    pub socket: PathBuf,
    /// `PATH` for the UI and its children.
    pub path_var: String,
    /// The compositor's Wayland socket. When it's gone after the UI exits,
    /// the session is over and novad exits.
    pub wayland_socket: Option<PathBuf>,
}

impl UiSettings {
    /// The program to start now: the one novad was given, else `nova-ui`
    /// on `PATH`, or why there is none.
    fn program_path(&self) -> Result<PathBuf, String> {
        match &self.program {
            Some(path) => Ok(path.clone()),
            None => nova_paths::find_program(GPUI_UI, &self.path_var)
                .ok_or_else(|| format!("{GPUI_UI} isn't on PATH, so Nova has no UI")),
        }
    }
}

pub struct Supervisor {
    record: Mutex<Record>,
    /// Signalled when `wanted` or `quitting` changes.
    changed: Condvar,
    quitting: AtomicBool,
}

/// How one run of the UI went.
enum Outcome {
    /// It ran and exited, as text for the log.
    Exited(String),
    /// It never started, and why.
    NotStarted(String),
    /// It wasn't wanted any more by the time it would have started.
    NotWanted,
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
    /// When the UI got to its state ([`UiStatus::since`]).
    since: Option<String>,
    /// Why it isn't running while wanted ([`UiStatus::problem`]).
    problem: Option<String>,
}

impl Supervisor {
    /// Starts the UI on a thread of its own that lives as long as novad,
    /// running it while `wanted` (see [`Self::want`]).
    pub fn start(settings: UiSettings, wanted: bool, logger: Arc<Logger>) -> Arc<Self> {
        if !wanted {
            logger.info(LOG_SOURCE, NOT_NEEDED);
        }
        let supervisor = Arc::new(Self {
            record: Mutex::new(Record { wanted, ..Record::default() }),
            changed: Condvar::new(),
            quitting: AtomicBool::new(false),
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
            program: UiProgram::Gpui,
            state,
            pid: record.pid,
            pss_kib: record
                .pid
                .and_then(|pid| nova_process::procfs::pss_kib(Path::new(nova_process::procfs::PROC), pid)),
            restarts: record.restarts,
            last_crash: record.last_crash.clone(),
            since: record.since.clone(),
            problem: record.problem.clone().filter(|_| state == UiState::Restarting),
        }
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
        // Why the last start failed, so the same reason is logged once.
        let mut failed_start: Option<String> = None;
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
            let outcome = self.run_once(settings, logger);
            if self.quitting.load(Ordering::SeqCst) {
                return;
            }
            if std::mem::take(&mut lock(&self.record).stopping) || matches!(outcome, Outcome::NotWanted) {
                // Stopped on purpose: no crash, and a fresh backoff.
                retry.reset();
                failed_start = None;
                self.not_running(None);
                continue;
            }
            exit_if_session_over(settings, logger);
            let ran_for = started.elapsed();
            match outcome {
                Outcome::Exited(exit) => {
                    failed_start = None;
                    self.not_running(Some(format!("it stopped after {} s: {exit}", ran_for.as_secs())));
                    self.record_crash(&exit, ran_for, logger);
                }
                Outcome::NotStarted(problem) => self.failed_to_start(problem, &mut failed_start, logger),
                Outcome::NotWanted => {}
            }
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

    /// Runs the UI until it exits, and says how that went.
    fn run_once(&self, settings: &UiSettings, logger: &Arc<Logger>) -> Outcome {
        // Spawned under the record lock, so a `want(false)` or `stop` either
        // comes first (and nothing starts) or sees the pid and ends it.
        let mut child = {
            let mut record = lock(&self.record);
            if !record.wanted || self.quitting.load(Ordering::SeqCst) {
                return Outcome::NotWanted;
            }
            let program = match settings.program_path() {
                Ok(program) => program,
                Err(problem) => return Outcome::NotStarted(problem),
            };
            match spawn(settings, &program) {
                Ok(child) => {
                    record.pid = Some(child.id());
                    record.since = Some(nova_log::now());
                    record.problem = None;
                    logger.info(LOG_SOURCE, &format!("UI started (pid {}) from {}", child.id(), program.display()));
                    child
                }
                Err(e) => return Outcome::NotStarted(format!("couldn't start {}: {e}", program.display())),
            }
        };
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
        Outcome::Exited(match status {
            Ok(status) => describe_exit(status),
            Err(e) => format!("couldn't wait for the UI: {e}"),
        })
    }

    /// Notes a start that failed for `problem`. It never ran: no output to
    /// keep, so no crash snapshot, and not a restart. The same reason as the
    /// last try's (`failed_start`) is an error only once; each try after it
    /// is the "starting again" info line.
    fn failed_to_start(&self, problem: String, failed_start: &mut Option<String>, logger: &Logger) {
        if failed_start.as_ref() == Some(&problem) {
            return;
        }
        logger.error(LOG_SOURCE, &format!("the UI didn't start: {problem}"));
        // Not running since the first failed try, not the latest.
        if failed_start.is_none() {
            self.not_running(None);
        }
        lock(&self.record).problem = Some(format!("it didn't start: {problem}"));
        *failed_start = Some(problem);
    }

    /// Notes that the UI stopped running now, and why, if it should run.
    fn not_running(&self, problem: Option<String>) {
        let mut record = lock(&self.record);
        record.since = Some(nova_log::now());
        record.problem = problem;
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

/// `program` as [`UI_PROCESS_NAME`], connected to novad, its output piped
/// to the log.
fn spawn(settings: &UiSettings, program: &Path) -> std::io::Result<Child> {
    let mut command = Command::new(program);
    command
        .arg0(UI_PROCESS_NAME)
        .env(SOCKET_VAR, &settings.socket)
        .env("PATH", &settings.path_var)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_given_wins_else_nova_ui_is_found_on_path_at_each_start() {
        let dir = nova_test_support::TempDir::new("gpui-pick");
        let settings = |program: Option<PathBuf>| UiSettings {
            program,
            socket: dir.join("socket"),
            path_var: dir.path_var(),
            wayland_socket: None,
        };
        let on_path = settings(None);
        assert_eq!(on_path.program_path(), Err("nova-ui isn't on PATH, so Nova has no UI".into()));
        // Installed after novad started: the next start finds it.
        let installed = nova_test_support::write_program(dir.path(), GPUI_UI, "exit 0");
        assert_eq!(on_path.program_path(), Ok(installed));
        let given = dir.join("given");
        assert_eq!(settings(Some(given.clone())).program_path(), Ok(given));
    }
}
