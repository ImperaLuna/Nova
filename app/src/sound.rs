use crate::Daemon;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, done};
use nova_sound::{FEATURE_ID, Sources};
use nova_sound_api::Command;
use nova_volume_osd_api::VolumeKey;
use std::sync::Arc;

pub struct Sound(Arc<nova_sound::Sound>);

pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match daemon.module::<Sound>() {
        Ok(sound) => {
            let command = command.clone();
            Answer::later(move || done(sound.0.change(&command)))
        }
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

pub fn press(daemon: &Daemon, key: VolumeKey) -> Answer {
    match daemon.module::<Sound>() {
        Ok(sound) => Answer::later(move || done(sound.0.press(key))),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

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
        let (hub, logger) = (Arc::clone(&ctx.hub), Arc::clone(&ctx.logger));
        Arc::new(Self(nova_sound::Sound::start(sources.clone(), hub, logger)))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
