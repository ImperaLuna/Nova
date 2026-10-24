//! Whether the session is locked (feature `lock`).
//!
//! The UI draws the lock screen and checks the password; novad only keeps
//! the state, so a UI that crashes while locked gets `locked: true` again when
//! its restart subscribes, and locks the screen again. Only the UI unlocks
//! (after the password checks out) and reports it here, so nothing on the
//! socket can unlock the screen.

use crate::hub::{ClientId, Hub};
use crate::lock;
use nova_protocol::Event;
use std::sync::Mutex;

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "lock";

#[derive(Default)]
pub struct SessionLock {
    locked: Mutex<bool>,
}

impl SessionLock {
    /// Tells the UI to lock the screen. Fails when no UI listens, since
    /// nothing would then cover the screen.
    pub fn lock(&self, hub: &Hub) -> Result<(), String> {
        // The state and its event change together, so a subscriber's
        // snapshot never misses a lock.
        let mut locked = lock(&self.locked);
        if hub.publish(&Event::Lock { locked: true }) == 0 {
            return Err("the UI isn't connected to novad, so nothing can lock the screen".into());
        }
        *locked = true;
        Ok(())
    }

    /// The UI unlocked the screen.
    pub fn unlocked(&self, hub: &Hub) {
        let mut locked = lock(&self.locked);
        if *locked {
            *locked = false;
            hub.publish(&Event::Lock { locked: false });
        }
    }

    /// Sends a new subscriber the current state. Under the same lock as
    /// changes, so it's never older than an event the subscriber got first.
    pub fn send_state(&self, hub: &Hub, id: ClientId) {
        let locked = lock(&self.locked);
        hub.send_to(id, &Event::Lock { locked: *locked });
    }
}
