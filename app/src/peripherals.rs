use nova_hub::ClientId;
use nova_notifications_api::Post;
use nova_peripherals::Sources;
use std::sync::Arc;

use nova_peripherals::FEATURE_ID;

pub struct Peripherals(Arc<nova_peripherals::Peripherals>);

impl nova_modules::Module for Peripherals {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_peripherals_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::system(env.path_var)
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(nova_peripherals::Peripherals::start(
            sources.clone(),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        )))
    }

    fn linked(&self, running: &crate::modules::Running) {
        let notifications = running.get::<crate::notifications::Notifications>();
        let post = notifications.map(|notifications| Arc::clone(notifications.core()) as Arc<dyn Post>);
        self.0.set_notifications(post.as_ref());
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
