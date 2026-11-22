//! Runs the media feature (`nova-media`, in `features/media`) in novad: its
//! module and the answers to its commands. Wiring only: it adapts the
//! feature's core to novad's `Module` trait and `Daemon`, fills its port
//! with the tray's process lookup, and all of the feature's work and state
//! stay in the core.

use crate::Daemon;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_media::Sources;
use nova_media_api::Command;
use nova_socket::{Answer, Caller, done, value};
use std::path::Path;
use std::sync::Arc;

pub use nova_media::FEATURE_ID;

const LOG_SOURCE: &str = "server";

/// The feature's core, as novad's module.
pub struct Media(Arc<nova_media::Media>);

impl std::ops::Deref for Media {
    type Target = nova_media::Media;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands from `caller`, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match command {
        Command::MediaShow { player } => {
            Answer::Now(done(daemon.module::<Media>().and_then(|media| media.show(player))))
        }
        Command::MediaControl { player, action } => {
            let (player, action) = (player.clone(), *action);
            later(daemon, move |media| done(media.control(&player, action)))
        }
        Command::MediaSeek { player, position_us } => {
            let (player, position_us) = (player.clone(), *position_us);
            later(daemon, move |media| done(media.seek(&player, position_us)))
        }
        Command::MediaFocus { player } => focus(daemon, player),
        Command::MediaWindow { player } => window(daemon, player),
    }
}

/// Runs `work` with the media service on its own thread: it waits on a
/// player, or the compositor, to answer.
fn later(daemon: &Daemon, work: impl FnOnce(&Media) -> Outcome + Send + 'static) -> Answer {
    match daemon.module::<Media>() {
        Ok(media) => Answer::later(move || work(&media)),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

/// Brings `player`'s window forward: where it is, as the launcher does
/// (IMP-114), else by asking the player to raise itself. Answers whether
/// either worked.
fn focus(daemon: &Daemon, player: &str) -> Answer {
    let compositor = daemon.compositor.clone();
    let logger = Arc::clone(&daemon.logger);
    let player = player.to_owned();
    later(daemon, move |media| {
        value(media.window_ids(&player).and_then(|ids| {
            let focused = match compositor.as_deref() {
                Some(compositor) if !ids.is_empty() => compositor.focus_app(&ids, None).unwrap_or_else(|e| {
                    logger.warn(LOG_SOURCE, &format!("can't focus {player}'s window: {e}"));
                    false
                }),
                _ => false,
            };
            if focused { Ok(true) } else { media.raise(&player) }
        }))
    })
}

/// The name of the workspace `player`'s window is on, or `null`.
fn window(daemon: &Daemon, player: &str) -> Answer {
    let compositor = daemon.compositor.clone();
    let player = player.to_owned();
    later(daemon, move |media| {
        value(media.window_ids(&player).and_then(|ids| match compositor.as_deref() {
            Some(compositor) if !ids.is_empty() => compositor.app_workspace(&ids),
            _ => Ok(None),
        }))
    })
}

/// The port: a player's process is looked up as the tray looks up a tray
/// icon's, with no icon id.
pub fn process_names(proc: &Path, pid: Option<u32>) -> Vec<String> {
    nova_tray::candidates(proc, pid, "")
}

/// No options.
impl nova_modules::Module for Media {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_media_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::new(env.runtime_dir, process_names)
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_protocol::FeatureState],
    ) -> Arc<Self> {
        let core = nova_media::Media::start(
            sources.clone(),
            Arc::clone(&ctx.apps),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        );
        Arc::new(Self(core))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}

#[cfg(test)]
mod tests {
    /// The process names the core's `a_player_is_found_by_its_process_...`
    /// test takes from the port are the ones the tray's lookup gives for
    /// Zen (`.zen-beta-wrapped`) started from a terminal (kitty's unit).
    #[test]
    fn media_finds_the_process_names_the_tray_does() {
        let dir = nova_test_support::TempDir::new("media-process-names");
        let process = dir.join("proc").join("4242");
        std::fs::create_dir_all(&process).unwrap();
        std::fs::write(process.join("comm"), ".zen-beta-wrappe\n").unwrap();
        // Started from a terminal: its unit is the terminal's.
        std::fs::write(process.join("cgroup"), "0::/user.slice/app.slice/app-kitty-4242.scope\n").unwrap();
        assert_eq!(super::process_names(&dir.join("proc"), Some(4242)), ["zen-beta", "kitty"]);
    }
}
