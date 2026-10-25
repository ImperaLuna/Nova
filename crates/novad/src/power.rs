//! The power menu's actions (feature `power`): lock, suspend, log out,
//! restart and shut down, and whether each can run now.
//!
//! Lock goes to the lock screen ([`crate::session_lock`]) and log out to the
//! compositor, which quits. Suspend, restart and shut down ask logind over
//! the system bus (systemd-logind, or elogind without systemd), the same
//! calls `loginctl` makes, so they need no root: logind and polkit allow them
//! for the active session. logind's `Can*` answers decide which ones the
//! menu dims.

use crate::logger::Logger;
use crate::logind::{MANAGER as LOGIND_MANAGER, PATH as LOGIND_PATH, SERVICE as LOGIND_SERVICE};
use nova_protocol::{PowerAction, PowerOption};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "power";
const LOG_SOURCE: &str = "power";

/// How long logind gets to connect and answer one call.
const LOGIND_TIMEOUT: Duration = Duration::from_secs(5);

/// The logind methods behind an action, `None` for actions logind doesn't run.
fn logind_methods(action: PowerAction) -> Option<(&'static str, &'static str)> {
    match action {
        PowerAction::Suspend => Some(("CanSuspend", "Suspend")),
        PowerAction::Reboot => Some(("CanReboot", "Reboot")),
        PowerAction::PowerOff => Some(("CanPowerOff", "PowerOff")),
        PowerAction::Lock | PowerAction::LogOut => None,
    }
}

/// What logind's `Can*` answer means for the menu, in a few words that fit
/// under its row; why logind couldn't answer goes to the log. "challenge"
/// means polkit would ask for a password first; the action stays, and polkit
/// says no if no agent asks.
fn from_can(action: PowerAction, answer: Result<String, String>, logger: &Logger) -> PowerOption {
    let (available, reason) = match answer.as_deref() {
        Ok("yes" | "challenge") => (true, None),
        Ok("no") => (false, Some("Not allowed for this session".to_string())),
        Ok("na") => (false, Some("Turned off on this system".to_string())),
        Ok(other) => (false, Some(format!("logind says {other:?}"))),
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("couldn't ask logind whether it can {action:?}: {e}"));
            (false, Some("Couldn't ask logind".to_string()))
        }
    };
    PowerOption { action, available, reason }
}

/// Every action, in the menu's order. `lock` and `log_out` say whether the
/// lock screen and the compositor can do theirs; logind answers the rest.
pub fn options(lock: Result<(), String>, log_out: Result<(), String>, logger: &Logger) -> Vec<PowerOption> {
    PowerAction::ALL
        .into_iter()
        .map(|action| {
            let local = match action {
                PowerAction::Lock => Some(&lock),
                PowerAction::LogOut => Some(&log_out),
                _ => None,
            };
            match (local, logind_methods(action)) {
                (Some(Ok(())), _) => PowerOption { action, available: true, reason: None },
                (Some(Err(reason)), _) => PowerOption { action, available: false, reason: Some(reason.clone()) },
                (None, Some((can, _))) => from_can(action, ask_logind(can, None), logger),
                (None, None) => unreachable!("every action is local or logind's"),
            }
        })
        .collect()
}

/// Runs a logind action (suspend, restart, shut down).
pub fn run_logind(action: PowerAction) -> Result<(), String> {
    let (_, method) = logind_methods(action).ok_or_else(|| format!("logind doesn't {action:?}"))?;
    // false: no polkit prompt; the menu has already asked the user.
    ask_logind(method, Some(false)).map(drop)
}

/// Calls `method` on logind's manager, with `interactive` as its one
/// argument when it takes one, on its own thread so the deadline holds even
/// while connecting. Returns a `Can*` method's answer, or "" for the others.
fn ask_logind(method: &'static str, interactive: Option<bool>) -> Result<String, String> {
    let (done, answer) = mpsc::sync_channel(1);
    // Set once the caller stops waiting, so a slow connection can't still
    // suspend or shut down after the menu was told it failed.
    let abandoned = Arc::new(AtomicBool::new(false));
    let too_late = Arc::clone(&abandoned);
    std::thread::spawn(move || {
        let result = zbus::block_on(async {
            let bus = zbus::Connection::system().await?;
            if too_late.load(Ordering::SeqCst) {
                return Err(zbus::Error::Failure(format!("gave up on {method} before calling it")));
            }
            let reply = match interactive {
                Some(interactive) => {
                    let body = (interactive,);
                    bus.call_method(Some(LOGIND_SERVICE), LOGIND_PATH, Some(LOGIND_MANAGER), method, &body).await?
                }
                None => bus.call_method(Some(LOGIND_SERVICE), LOGIND_PATH, Some(LOGIND_MANAGER), method, &()).await?,
            };
            match interactive {
                Some(_) => Ok(String::new()),
                None => reply.body().deserialize::<String>(),
            }
        });
        let _ = done.send(result.map_err(|e| e.to_string()));
    });
    answer.recv_timeout(LOGIND_TIMEOUT).map_err(|_| {
        abandoned.store(true, Ordering::SeqCst);
        format!("logind didn't answer {method} within {} s", LOGIND_TIMEOUT.as_secs())
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Hub;
    use nova_protocol::LogLevel;
    use std::sync::Arc;

    #[test]
    fn logind_answers_dim_what_cant_run() {
        let dir = std::env::temp_dir().join(format!("novad-power-{}", std::process::id()));
        let logger = Logger::open(&dir, LogLevel::Info, false, Arc::new(Hub::default())).unwrap();
        let option = |answer: Result<&str, &str>| {
            from_can(PowerAction::Suspend, answer.map(str::to_string).map_err(str::to_string), &logger)
        };
        assert!(option(Ok("yes")).available);
        assert!(option(Ok("challenge")).available, "polkit may still allow it");
        for answer in [Ok("no"), Ok("na"), Ok("later"), Err("no system bus")] {
            let dimmed = option(answer);
            assert!(!dimmed.available, "{answer:?}");
            assert!(dimmed.reason.is_some_and(|reason| !reason.is_empty()), "{answer:?} says why");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_suspend_restart_and_shut_down_are_loginds() {
        for action in PowerAction::ALL {
            let local = matches!(action, PowerAction::Lock | PowerAction::LogOut);
            assert_eq!(logind_methods(action).is_none(), local, "{action:?}");
        }
    }
}
