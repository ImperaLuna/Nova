//! Runs the sound feature (`nova-sound`, in `features/sound`) in novad: its
//! module and the answers to its commands. Wiring only: it adapts the
//! feature's core to novad's `Module` trait and `Daemon`, and all of the
//! feature's work and state stay in the core.

use crate::Daemon;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, done};
use nova_sound::{FEATURE_ID, Sources};
use nova_sound_api::Command;
use std::sync::Arc;

/// The feature's core, as novad's module.
pub struct Sound(Arc<nova_sound::Sound>);

/// The answer to one of the feature's commands, the route
/// `requests::router` gives them: on its own thread, since it waits on the
/// sound server.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match daemon.module::<Sound>() {
        Ok(sound) => {
            let command = command.clone();
            Answer::later(move || done(sound.0.change(&command)))
        }
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

/// It has no options: it only starts and stops with its feature.
impl nova_modules::Module for Sound {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_sound_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(_: &crate::modules::Env) -> Sources {
        Sources::system()
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(nova_sound::Sound::start(sources.clone(), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
