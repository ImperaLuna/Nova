//! The lock feature's messages: locking the session, and the UI telling
//! novad what its lock screen did.
//!
//! A client sends a [`Command`] (`lock`, `report_locked`,
//! `report_unlocked`); novad publishes an [`Event`] on [`TOPIC`] right after
//! subscribing and whenever the state changes. The work is `nova-lock`
//! (`features/lock/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Lock`].
pub const TOPIC: Topic = Topic::named("lock");

/// Sent by a client. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Locks the session (`nova lock`, feature `lock`) and answers once the
    /// compositor has covered the screens. Fails when the feature isn't
    /// ready, no UI is connected to show the lock screen, or the screens
    /// aren't covered in time.
    Lock,
    /// The compositor covered the screens with the UI's lock screen
    /// (feature `lock`).
    ReportLocked,
    /// The UI unlocked the session after the user's password checked out
    /// (feature `lock`). Unlocking is the UI's alone: this only tells novad.
    ReportUnlocked,
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Whether the session is locked (feature `lock`). Topic [`TOPIC`],
    /// sent right after subscribing and whenever it changes. The UI locks
    /// when `locked` turns true, but only ever unlocks itself, after the
    /// user's password checks out: `locked: false` just follows that.
    Lock { locked: bool },
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_event_is_the_lock_topics() {
        assert_eq!(Event::Lock { locked: true }.topic(), Some(TOPIC));
        let line = serde_json::to_string(&Event::Lock { locked: true }).unwrap();
        assert_eq!(line, r#"{"event":"lock","data":{"locked":true}}"#);
    }
}
