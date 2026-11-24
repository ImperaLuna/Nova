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

pub struct Media(Arc<nova_media::Media>);

impl std::ops::Deref for Media {
    type Target = nova_media::Media;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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

fn later(daemon: &Daemon, work: impl FnOnce(&Media) -> Outcome + Send + 'static) -> Answer {
    match daemon.module::<Media>() {
        Ok(media) => Answer::later(move || work(&media)),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

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

pub fn process_names(proc: &Path, pid: Option<u32>) -> Vec<String> {
    nova_tray::candidates(proc, pid, "")
}

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
        _: &[nova_daemon_info::FeatureState],
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
    #[test]
    fn media_finds_the_process_names_the_tray_does() {
        let dir = nova_test_support::TempDir::new("media-process-names");
        let process = dir.join("proc").join("4242");
        std::fs::create_dir_all(&process).unwrap();
        std::fs::write(process.join("comm"), ".zen-beta-wrappe\n").unwrap();
        std::fs::write(process.join("cgroup"), "0::/user.slice/app.slice/app-kitty-4242.scope\n").unwrap();
        assert_eq!(super::process_names(&dir.join("proc"), Some(4242)), ["zen-beta", "kitty"]);
    }
}
