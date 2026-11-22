//! Runs the sound feature (`nova-sound`, in `features/sound`) in novad: its
//! module. Wiring only: it adapts the feature's core to novad's `Module`
//! trait, and all of the feature's work and state stay in the core.

use nova_hub::ClientId;
use nova_sound::{FEATURE_ID, Sources};
use std::sync::Arc;

/// The feature's core, as novad's module.
pub struct Sound(Arc<nova_sound::Sound>);

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
