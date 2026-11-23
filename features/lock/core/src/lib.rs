use nova_base::lock;
use nova_hub::{ClientId, Hub};
use nova_lock_api::Event;
use nova_log::Logger;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub const FEATURE_ID: &str = "lock";
const LOG_SOURCE: &str = "lock";
const COVER_TIMEOUT: Duration = Duration::from_secs(5);
const LOCKED_FILE: &str = "locked";

pub struct SessionLock {
    state: Mutex<State>,
    covered: Condvar,
    file: PathBuf,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
}

#[derive(Default)]
struct State {
    locked: bool,
    secured: bool,
}

impl SessionLock {
    pub fn open(runtime_dir: &Path, hub: Arc<Hub>, logger: &Arc<Logger>) -> Self {
        let file = runtime_dir.join(LOCKED_FILE);
        let locked = file.exists();
        if locked {
            logger.info(LOG_SOURCE, "the session was locked when novad last stopped; locking again");
        }
        Self {
            state: Mutex::new(State { locked, secured: false }),
            covered: Condvar::new(),
            file,
            hub,
            logger: Arc::clone(logger),
        }
    }

    pub fn lock(&self, hub: &Hub) -> Result<(), String> {
        self.request(hub)?;
        self.wait_covered()
    }

    pub fn request(&self, hub: &Hub) -> Result<(), String> {
        let mut state = lock(&self.state);
        let listening = hub.publish(&Event::Lock { locked: true });
        if state.locked && state.secured {
            return Ok(());
        }
        if listening == 0 {
            return Err("the UI isn't connected to novad, so nothing can lock the screen".into());
        }
        if !state.locked {
            state.locked = true;
            self.save(true);
        }
        Ok(())
    }

    pub fn wait_covered(&self) -> Result<(), String> {
        let state = lock(&self.state);
        let (state, wait) = self
            .covered
            .wait_timeout_while(state, COVER_TIMEOUT, |state| state.locked && !state.secured)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match (state.secured, wait.timed_out()) {
            (true, _) => Ok(()),
            (false, true) => Err(format!("the screens weren't covered within {} s", COVER_TIMEOUT.as_secs())),
            (false, false) => Err("the screen was unlocked before it was covered".into()),
        }
    }

    pub fn secured(&self) {
        let mut state = lock(&self.state);
        if state.locked {
            state.secured = true;
            self.covered.notify_all();
        }
    }

    pub fn unlocked(&self, hub: &Hub) {
        let mut state = lock(&self.state);
        if state.locked {
            *state = State::default();
            self.save(false);
            self.covered.notify_all();
            hub.publish(&Event::Lock { locked: false });
        }
    }

    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(id, &Event::Lock { locked: state.locked });
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
    use nova_carrier::log::LogLevel;

    #[test]
    fn a_restarted_novad_locks_again_until_unlocked() {
        let dir = nova_test_support::TempDir::new("session-lock");
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Info, false, Arc::clone(&hub)).unwrap());
        assert!(!lock(&SessionLock::open(&dir, Arc::new(Hub::default()), &logger).state).locked);

        std::fs::write(dir.join(LOCKED_FILE), "").unwrap();
        let restarted = SessionLock::open(&dir, Arc::new(Hub::default()), &logger);
        assert!(lock(&restarted.state).locked);
        restarted.unlocked(&hub);
        assert!(!SessionLock::open(&dir, Arc::new(Hub::default()), &logger).state.lock().unwrap().locked);
    }
}
