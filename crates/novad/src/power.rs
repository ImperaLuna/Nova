//! Runs the power feature (`nova-power`, in `features/power`) in novad: the
//! answers to its commands and following logind for the lock screen.
//! Wiring only: it adapts the feature's core to novad's `Daemon`. The
//! feature keeps no state, so it has no module; its commands answer while
//! its feature is ready.

use crate::{Daemon, features, session_lock};
use nova_power_api::{Command, PowerAction, PowerMenu};
use nova_protocol::{Capability, Outcome};
use nova_socket::{Answer, Caller, done, value};
use std::sync::Arc;

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "power";
const LOG_SOURCE: &str = "server";

/// The answer to one of the feature's commands, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match command {
        Command::PowerOptions => options(daemon),
        Command::Power { action } => power(daemon, *action),
    }
}

/// An error saying the feature `id` isn't ready, when it isn't.
fn require_feature(daemon: &Daemon, id: &str) -> Result<(), String> {
    match daemon.feature_ready(id) {
        true => Ok(()),
        false => Err(features::not_ready(id)),
    }
}

fn options(daemon: &Daemon) -> Answer {
    if let Err(e) = require_feature(daemon, FEATURE_ID) {
        return Answer::Now(Outcome::Error(e));
    }
    let lock = require_feature(daemon, session_lock::FEATURE_ID);
    let log_out = match &daemon.compositor {
        Some(compositor) if compositor.can(Capability::LogOut) => Ok(()),
        _ => Err("Not on this compositor".into()),
    };
    let logger = Arc::clone(&daemon.logger);
    Answer::later(move || {
        let menu =
            PowerMenu { options: nova_power::options(lock, log_out, &logger), booted_at: crate::procfs::boot_time() };
        value(Ok(menu))
    })
}

/// Runs a power menu action. The menu asks over the UI's own
/// connection, which can't report the lock screen covering the screens
/// while this waits, so locking here doesn't wait for it.
fn power(daemon: &Daemon, action: PowerAction) -> Answer {
    if let Err(e) = require_feature(daemon, FEATURE_ID) {
        return Answer::Now(Outcome::Error(e));
    }
    daemon.logger.info(LOG_SOURCE, &format!("power menu: {action:?}"));
    match action {
        PowerAction::Lock => Answer::Now(done(
            require_feature(daemon, session_lock::FEATURE_ID).and_then(|()| daemon.session_lock().request(&daemon.hub)),
        )),
        PowerAction::LogOut => Answer::Now(done(
            daemon
                .compositor
                .as_deref()
                .ok_or_else(|| "novad doesn't know this compositor".to_string())
                .and_then(crate::compositor::Compositor::log_out),
        )),
        PowerAction::Suspend | PowerAction::Reboot | PowerAction::PowerOff => logind(daemon, action),
    }
}

/// Suspend, restart or shut down through logind; answers with logind's
/// own result. A suspend locks the screen first through novad's sleep
/// inhibitor ([`nova_power::logind`]), as one from anywhere else does.
fn logind(daemon: &Daemon, action: PowerAction) -> Answer {
    let logger = Arc::clone(&daemon.logger);
    Answer::later(move || {
        done(
            nova_power::run_logind(action).inspect_err(|e| logger.warn(LOG_SOURCE, &format!("{action:?} failed: {e}"))),
        )
    })
}

/// Locks the screen when logind asks, and before the machine sleeps, for
/// as long as novad runs.
pub fn follow_logind(daemon: &Arc<Daemon>) {
    nova_power::follow(Arc::new(LockScreen(Arc::clone(daemon))), Arc::clone(&daemon.logger));
}

/// novad's lock screen, as the feature's [`nova_power::LockScreen`] port.
struct LockScreen(Arc<Daemon>);

impl nova_power::LockScreen for LockScreen {
    fn ready(&self) -> bool {
        self.0.feature_ready(session_lock::FEATURE_ID)
    }

    fn request(&self) -> Result<(), String> {
        self.0.session_lock().request(&self.0.hub)
    }

    fn wait_covered(&self) -> Result<(), String> {
        self.0.session_lock().wait_covered()
    }
}
