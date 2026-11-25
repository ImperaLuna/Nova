pub mod logind;

pub use logind::{LockScreen, follow};
use logind::{MANAGER as LOGIND_MANAGER, PATH as LOGIND_PATH, SERVICE as LOGIND_SERVICE};
use nova_log::Logger;
use nova_power_api::{PowerAction, PowerOption};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub const FEATURE_ID: &str = "power";
const LOG_SOURCE: &str = "power";

const LOGIND_TIMEOUT: Duration = Duration::from_secs(5);

fn logind_methods(action: PowerAction) -> Option<(&'static str, &'static str)> {
    match action {
        PowerAction::Suspend => Some(("CanSuspend", "Suspend")),
        PowerAction::Reboot => Some(("CanReboot", "Reboot")),
        PowerAction::PowerOff => Some(("CanPowerOff", "PowerOff")),
        PowerAction::Lock | PowerAction::LogOut => None,
    }
}

fn from_can(action: PowerAction, answer: Result<String, String>, logger: &Logger) -> PowerOption {
    let (available, reason) = match answer.as_deref() {
        Ok("yes" | "challenge") => (true, None),
        Ok("no") => (false, Some("Not allowed".to_string())),
        Ok("na") => (false, Some("Turned off".to_string())),
        Ok(other) => (false, Some(format!("logind says {other:?}"))),
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("couldn't ask logind whether it can {action:?}: {e}"));
            (false, Some("Couldn't ask logind".to_string()))
        }
    };
    PowerOption { action, available, reason }
}

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

pub fn run_logind(action: PowerAction) -> Result<(), String> {
    let (_, method) = logind_methods(action).ok_or_else(|| format!("logind doesn't {action:?}"))?;
    ask_logind(method, Some(false)).map(drop)
}

fn ask_logind(method: &'static str, interactive: Option<bool>) -> Result<String, String> {
    let (done, answer) = mpsc::sync_channel(1);
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
    use nova_carrier::log::LogLevel;
    use nova_hub::Hub;
    use std::sync::Arc;

    #[test]
    fn logind_answers_dim_what_cant_run() {
        let dir = nova_test_support::TempDir::new("power");
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
    }

    #[test]
    fn only_suspend_restart_and_shut_down_are_loginds() {
        for action in PowerAction::ALL {
            let local = matches!(action, PowerAction::Lock | PowerAction::LogOut);
            assert_eq!(logind_methods(action).is_none(), local, "{action:?}");
        }
    }
}
