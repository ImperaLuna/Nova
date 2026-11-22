//! Runs the lock feature (`nova-lock`, in `features/lock`) in novad: its
//! kept module and the answers to its commands. Wiring only: it adapts the
//! feature's core to novad's `Module` trait and `Daemon`, and all of the
//! feature's work and state stay in the core.

use crate::Daemon;
use nova_hub::ClientId;
use nova_lock_api::Command;
use nova_socket::{Answer, Caller, done};
use std::path::PathBuf;
use std::sync::Arc;

use nova_lock::FEATURE_ID;

/// The feature's core, as novad's module.
pub struct SessionLock(nova_lock::SessionLock);

impl std::ops::Deref for SessionLock {
    type Target = nova_lock::SessionLock;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands from `caller`, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, caller: &Caller, command: &Command) -> Answer {
    let session_lock = daemon.session_lock();
    Answer::Now(match command {
        Command::Lock => done(match daemon.feature_ready(FEATURE_ID) {
            true => session_lock.lock(&daemon.hub),
            false => Err(nova_modules::not_ready(FEATURE_ID)),
        }),
        Command::ReportLocked => done(sent_by_the_ui(daemon, caller).map(|()| session_lock.secured())),
        // Even with the feature off: the screen was locked while it was on.
        Command::ReportUnlocked => done(sent_by_the_ui(daemon, caller).map(|()| session_lock.unlocked(&daemon.hub))),
    })
}

/// Lock reports are the UI's alone: from anyone else, `report_unlocked`
/// would leave a UI that restarts with no lock screen to unlock. When
/// novad runs no UI (`--no-ui`: tests, or a UI run by hand), it can't
/// tell which client is the UI and takes them from any.
fn sent_by_the_ui(daemon: &Daemon, caller: &Caller) -> Result<(), String> {
    let Some(ui) = &daemon.ui else { return Ok(()) };
    match ui.status().pid {
        // The connecting process, from the kernel (SO_PEERCRED).
        Some(pid) if caller.peer_pid() == Some(pid) => Ok(()),
        _ => Err("only the UI novad runs can report the lock screen's state".into()),
    }
}

/// Kept for novad's life: the screen stays locked while the feature is off.
impl nova_modules::Module for SessionLock {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_lock_api::TOPIC);
    type Sources = PathBuf;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> PathBuf {
        env.runtime_dir.to_owned()
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        runtime_dir: &PathBuf,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(nova_lock::SessionLock::open(runtime_dir, Arc::clone(&ctx.hub), &ctx.logger)))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {}
}
