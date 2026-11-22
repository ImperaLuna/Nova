//! The power feature's messages: what the power menu can do now and the
//! actions it runs (lock, log out, suspend, restart, shut down).
//!
//! A client sends a [`Command`] (`power_options`, `power`); the feature has
//! no events and no topic. The work is `nova-power` (`features/power/core`).

use serde::{Deserialize, Serialize};

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Replies with a [`PowerMenu`]: what the power menu can do now, a
    /// [`PowerOption`] per [`PowerAction`], and when the system booted
    /// (feature `power`). Asked each time the menu opens.
    PowerOptions,
    /// Runs a power menu action (feature `power`). The menu asks the user
    /// to confirm the ones that end the session first.
    Power { action: PowerAction },
}

/// What the power menu does, in the order it lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    Lock,
    LogOut,
    Suspend,
    Reboot,
    PowerOff,
}

impl PowerAction {
    pub const ALL: [PowerAction; 5] =
        [PowerAction::Lock, PowerAction::LogOut, PowerAction::Suspend, PowerAction::Reboot, PowerAction::PowerOff];
}

/// Whether a power action can run now, and why not when it can't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerOption {
    pub action: PowerAction,
    pub available: bool,
    /// For people, shown under the dimmed action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What `power_options` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerMenu {
    pub options: Vec<PowerOption>,
    /// When the system booted, in Unix seconds, for the menu's uptime; left
    /// out when novad can't tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub booted_at: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_menu_rejects_unknown_fields() {
        let menu = r#"{"options":[{"action":"lock","available":true}],"booted_at":1}"#;
        assert!(serde_json::from_str::<PowerMenu>(menu).is_ok());
        assert!(serde_json::from_str::<PowerMenu>(r#"{"options":[],"extra":1}"#).is_err());
        let option = r#"{"options":[{"action":"lock","available":true,"extra":1}]}"#;
        assert!(serde_json::from_str::<PowerMenu>(option).is_err());
    }
}
