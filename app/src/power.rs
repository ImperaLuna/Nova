use crate::Daemon;
use nova_carrier::Outcome;
use nova_desktop_info::Capability;
use nova_power_api::{Command, PowerAction, PowerMenu};
use nova_socket::{Answer, Caller, done, value};
use std::sync::Arc;

use nova_power::FEATURE_ID;
const LOG_SOURCE: &str = "server";

pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match command {
        Command::PowerOptions => options(daemon),
        Command::Power { action } => power(daemon, *action),
    }
}

fn require_feature(daemon: &Daemon, id: &str) -> Result<(), String> {
    match daemon.feature_ready(id) {
        true => Ok(()),
        false => Err(nova_modules::not_ready(id)),
    }
}

fn options(daemon: &Daemon) -> Answer {
    if let Err(e) = require_feature(daemon, FEATURE_ID) {
        return Answer::Now(Outcome::Error(e));
    }
    let lock = require_feature(daemon, nova_lock::FEATURE_ID);
    let log_out = match &daemon.compositor {
        Some(compositor) if compositor.can(Capability::LogOut) => Ok(()),
        _ => Err("Not on this compositor".into()),
    };
    let logger = Arc::clone(&daemon.logger);
    Answer::later(move || {
        let menu = PowerMenu {
            options: nova_power::options(lock, log_out, &logger),
            booted_at: nova_process::procfs::boot_time(),
        };
        value(Ok(menu))
    })
}

fn power(daemon: &Daemon, action: PowerAction) -> Answer {
    if let Err(e) = require_feature(daemon, FEATURE_ID) {
        return Answer::Now(Outcome::Error(e));
    }
    daemon.logger.info(LOG_SOURCE, &format!("power menu: {action:?}"));
    match action {
        PowerAction::Lock => Answer::Now(done(
            require_feature(daemon, nova_lock::FEATURE_ID).and_then(|()| daemon.session_lock().request(&daemon.hub)),
        )),
        PowerAction::LogOut => Answer::Now(done(
            daemon
                .compositor
                .as_deref()
                .ok_or_else(|| "novad doesn't know this compositor".to_string())
                .and_then(nova_compositor::Compositor::log_out),
        )),
        PowerAction::Suspend | PowerAction::Reboot | PowerAction::PowerOff => logind(daemon, action),
    }
}

fn logind(daemon: &Daemon, action: PowerAction) -> Answer {
    let logger = Arc::clone(&daemon.logger);
    Answer::later(move || {
        done(
            nova_power::run_logind(action).inspect_err(|e| logger.warn(LOG_SOURCE, &format!("{action:?} failed: {e}"))),
        )
    })
}

pub fn follow_logind(daemon: &Arc<Daemon>) {
    nova_power::follow(Arc::new(LockScreen(Arc::clone(daemon))), Arc::clone(&daemon.logger));
}

struct LockScreen(Arc<Daemon>);

impl nova_power::LockScreen for LockScreen {
    fn ready(&self) -> bool {
        self.0.feature_ready(nova_lock::FEATURE_ID)
    }

    fn request(&self) -> Result<(), String> {
        self.0.session_lock().request(&self.0.hub)
    }

    fn wait_covered(&self) -> Result<(), String> {
        self.0.session_lock().wait_covered()
    }
}
