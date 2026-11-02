//! The Nova Files process: Quickshell running `ui/files.qml`, which shows
//! every Nova Files window. It's started when a window is asked for and
//! none is running, and quits on its own when its last window closes, so
//! it never counts towards the shell's memory while no window is open and
//! a crash in it can't take the bar down. Its output goes into novad's log
//! as the files module's.
//!
//! It's started from a thread of its own that lives as long as the module
//! ([`child::die_with_novad`] ties a child to the thread that spawned it):
//! stopping the module, or novad going away, ends it with its windows.

use crate::logger::Logger;
use crate::supervisor::{self, UiSettings};
use crate::{child, lock};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Its process name in ps and btop.
pub const PROCESS_NAME: &str = "NovaFiles";
/// Its shell, in the UI's folder, so it imports the theme and the daemon
/// connection like the shell does.
pub const ENTRY: &str = "files.qml";

/// The process, while it runs.
pub struct Process {
    /// Asks the starter thread to start it; dropped to end that thread.
    starts: Mutex<Option<Sender<()>>>,
    pid: Arc<Mutex<Running>>,
}

/// Under one lock: no other process that gets the pid later is ever
/// signalled, and none starts once stopped.
#[derive(Default)]
struct Running {
    /// Its pid while it runs. Forgotten before it's reaped, and only
    /// signalled under the lock.
    pid: Option<u32>,
    /// Stopped: start requests still queued are dropped.
    stopped: bool,
}

/// Told when the process exits, after its pid is forgotten.
type Exited = Arc<dyn Fn() + Send + Sync>;

impl Process {
    /// Ready to start Nova Files with `settings` when asked to, calling
    /// `exited` each time it exits.
    pub fn new(settings: UiSettings, logger: Arc<Logger>, exited: impl Fn() + Send + Sync + 'static) -> Self {
        let (starts, asked) = mpsc::channel();
        let pid = Arc::new(Mutex::new(Running::default()));
        let running = Arc::clone(&pid);
        let exited: Exited = Arc::new(exited);
        std::thread::spawn(move || run(&settings, &asked, &running, &logger, &exited));
        Self { starts: Mutex::new(Some(starts)), pid }
    }

    /// Starts it unless it runs already.
    pub fn ensure_running(&self) {
        if let Some(starts) = &*lock(&self.starts) {
            let _ = starts.send(());
        }
    }

    /// Ends it, and starts no more.
    pub fn stop(&self) {
        lock(&self.starts).take();
        let mut running = lock(&self.pid);
        running.stopped = true;
        if let Some(pid) = running.pid {
            child::terminate(pid);
        }
    }
}

/// Starts the process each time it's asked to while none runs, until the
/// asking ends. Its children get SIGTERM when this thread ends.
fn run(settings: &UiSettings, asked: &Receiver<()>, pid: &Arc<Mutex<Running>>, logger: &Arc<Logger>, exited: &Exited) {
    for () in asked {
        let mut running = lock(pid);
        if running.pid.is_none() && !running.stopped {
            running.pid = start(settings, pid, logger, exited);
        }
    }
}

/// Starts it, logging its output, and forgets its pid once it exits.
/// Returns its pid, or `None` when it couldn't start (logged).
fn start(settings: &UiSettings, pid: &Arc<Mutex<Running>>, logger: &Arc<Logger>, exited: &Exited) -> Option<u32> {
    let mut command = supervisor::quickshell_command(settings, &settings.ui_dir.join(ENTRY), PROCESS_NAME);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    child::die_with_novad(&mut command, libc::SIGTERM);
    let mut process = match command.spawn() {
        Ok(process) => process,
        Err(e) => {
            logger.warn(super::LOG_SOURCE, &format!("couldn't start {}: {e}", settings.quickshell.display()));
            return None;
        }
    };
    let started = process.id();
    logger.info(super::LOG_SOURCE, &format!("Nova Files started (pid {started})"));
    if let Some(out) = process.stdout.take() {
        supervisor::forward_lines(out, Arc::clone(logger), super::LOG_SOURCE);
    }
    if let Some(err) = process.stderr.take() {
        supervisor::forward_lines(err, Arc::clone(logger), super::LOG_SOURCE);
    }
    let forget = Arc::clone(pid);
    let logger = Arc::clone(logger);
    let exited = Arc::clone(exited);
    std::thread::spawn(move || {
        // Waited for without reaping, so the pid is forgotten first.
        let waited = child::wait_for_exit(started);
        lock(&forget).pid = None;
        let ended = match waited.and_then(|()| process.wait()) {
            Ok(status) => status.to_string(),
            Err(e) => {
                child::kill_and_reap(&mut process);
                format!("couldn't wait for it: {e}")
            }
        };
        logger.info(super::LOG_SOURCE, &format!("Nova Files exited: {ended}"));
        exited();
    });
    Some(started)
}
