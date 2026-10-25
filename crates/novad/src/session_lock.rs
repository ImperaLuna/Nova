//! Whether the session is locked (feature `lock`).
//!
//! The UI draws the lock screen and checks the password; novad only keeps
//! the state, so a UI that crashes while locked gets `locked: true` again when
//! its restart subscribes, and locks the screen again. The state is also kept
//! in a file beside the socket, so a novad that restarts while locked (and
//! its UI with it) locks again too: the compositor keeps the screens covered
//! meanwhile. The socket's folder is in the runtime directory, which logging
//! out or rebooting empties, so the next session doesn't start locked. Only the UI unlocks (after the password checks out) and
//! reports it here, so nothing on the socket can unlock the screen.

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use nova_protocol::Event;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "lock";
const LOG_SOURCE: &str = "lock";
/// How long the UI and the compositor get to cover the screens before
/// `nova lock` gives up.
const COVER_TIMEOUT: Duration = Duration::from_secs(5);
/// There while the session is locked, beside the socket.
const LOCKED_FILE: &str = "locked";

pub struct SessionLock {
    state: Mutex<State>,
    covered: Condvar,
    file: PathBuf,
    logger: Arc<Logger>,
}

#[derive(Default)]
struct State {
    locked: bool,
    // The compositor has covered the screens for this lock.
    secured: bool,
}

impl SessionLock {
    /// Locked if the last novad of this session was, as its file in
    /// `runtime_dir` (the socket's folder) says.
    pub fn open(runtime_dir: &Path, logger: &Arc<Logger>) -> Self {
        let file = runtime_dir.join(LOCKED_FILE);
        let locked = file.exists();
        if locked {
            logger.info(LOG_SOURCE, "the session was locked when novad last stopped; locking again");
        }
        Self {
            state: Mutex::new(State { locked, secured: false }),
            covered: Condvar::new(),
            file,
            logger: Arc::clone(logger),
        }
    }

    /// Tells the UI to lock the screen, and waits until the compositor has
    /// covered it, so a caller (an idle daemon before sleep) can count on it.
    /// Fails when no UI listens or the screens aren't covered in time; the
    /// session stays locked then, for a UI that's only slow.
    pub fn lock(&self, hub: &Hub) -> Result<(), String> {
        // The state and its event change together, so a subscriber's
        // snapshot never misses a lock.
        let mut state = lock(&self.state);
        if state.locked && state.secured {
            return Ok(());
        }
        if hub.publish(&Event::Lock { locked: true }) == 0 {
            return Err("the UI isn't connected to novad, so nothing can lock the screen".into());
        }
        if !state.locked {
            state.locked = true;
            self.save(true);
        }
        let (state, wait) = self
            .covered
            .wait_timeout_while(state, COVER_TIMEOUT, |state| state.locked && !state.secured)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match (state.locked, wait.timed_out()) {
            (true, true) => Err(format!("the screens weren't covered within {} s", COVER_TIMEOUT.as_secs())),
            _ => Ok(()),
        }
    }

    /// The compositor covered the screens with the UI's lock screen.
    pub fn secured(&self) {
        let mut state = lock(&self.state);
        if state.locked {
            state.secured = true;
            self.covered.notify_all();
        }
    }

    /// The UI unlocked the screen.
    pub fn unlocked(&self, hub: &Hub) {
        let mut state = lock(&self.state);
        if state.locked {
            *state = State::default();
            self.save(false);
            self.covered.notify_all();
            hub.publish(&Event::Lock { locked: false });
        }
    }

    /// Sends a new subscriber the current state. Under the same lock as
    /// changes, so it's never older than an event the subscriber got first.
    /// A UI that subscribes while locked covers the screens anew.
    pub fn send_state(&self, hub: &Hub, id: ClientId) {
        let mut state = lock(&self.state);
        state.secured = false;
        hub.send_to(id, &Event::Lock { locked: state.locked });
    }

    fn save(&self, locked: bool) {
        let saved = match locked {
            true => std::fs::write(&self.file, ""),
            false => std::fs::remove_file(&self.file),
        };
        if let Err(e) = saved {
            self.logger.warn(LOG_SOURCE, &format!("couldn't save the lock state in {}: {e}", self.file.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::LogLevel;

    #[test]
    fn a_restarted_novad_locks_again_until_unlocked() {
        let dir = std::env::temp_dir().join(format!("novad-session-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Info, false, Arc::clone(&hub)).unwrap());
        assert!(!lock(&SessionLock::open(&dir, &logger).state).locked);

        std::fs::write(dir.join(LOCKED_FILE), "").unwrap();
        let restarted = SessionLock::open(&dir, &logger);
        assert!(lock(&restarted.state).locked);
        restarted.unlocked(&hub);
        assert!(!SessionLock::open(&dir, &logger).state.lock().unwrap().locked);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
