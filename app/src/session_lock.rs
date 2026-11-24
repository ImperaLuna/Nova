use crate::Daemon;
use nova_hub::ClientId;
use nova_lock_api::Command;
use nova_socket::{Answer, Caller, done};
use std::path::PathBuf;
use std::sync::Arc;

use nova_lock::FEATURE_ID;

pub struct SessionLock(nova_lock::SessionLock);

impl std::ops::Deref for SessionLock {
    type Target = nova_lock::SessionLock;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, caller: &Caller, command: &Command) -> Answer {
    let session_lock = daemon.session_lock();
    Answer::Now(match command {
        Command::Lock => done(match daemon.feature_ready(FEATURE_ID) {
            true => session_lock.lock(&daemon.hub),
            false => Err(nova_modules::not_ready(FEATURE_ID)),
        }),
        Command::ReportLocked => done(sent_by_the_ui(daemon, caller).map(|()| session_lock.secured())),
        Command::ReportUnlocked => done(sent_by_the_ui(daemon, caller).map(|()| session_lock.unlocked(&daemon.hub))),
    })
}

fn sent_by_the_ui(daemon: &Daemon, caller: &Caller) -> Result<(), String> {
    let Some(ui) = &daemon.ui else { return Ok(()) };
    match ui.status().pid {
        Some(pid) if caller.peer_pid() == Some(pid) => Ok(()),
        _ => Err("only the UI novad runs can report the lock screen's state".into()),
    }
}

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
