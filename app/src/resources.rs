//! Runs the resources feature (`nova-resources`, in `features/resources`)
//! in novad: its module and the answers to its commands. Wiring only: it
//! adapts the feature's core to novad's `Module` trait and `Daemon`, and
//! all of the feature's work and state stay in the core.

use crate::Daemon;
use nova_hub::ClientId;
use nova_resources::Sources;
use nova_resources_api::Command;
use nova_socket::{Answer, Caller, done};
use std::sync::Arc;

use nova_resources::FEATURE_ID;

/// The feature's core, as novad's module.
pub struct Resources(Arc<nova_resources::Resources>);

impl std::ops::Deref for Resources {
    type Target = nova_resources::Resources;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands from `caller`, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, caller: &Caller, command: &Command) -> Answer {
    let resources = daemon.module::<Resources>();
    Answer::Now(match command {
        Command::WatchResources { watching } => {
            done(resources.map(|resources| resources.watch(caller.id(), *watching)))
        }
        Command::EndApp { key } => done(resources.and_then(|resources| resources.end_app(key))),
    })
}

/// It has no options: it only starts and stops with its feature.
impl nova_modules::Module for Resources {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    // It sends nothing on subscribe: readings come while some client watches.
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_resources_api::TOPIC);
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
        Arc::new(Self(nova_resources::Resources::start(sources.clone(), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))))
    }

    fn client_gone(&self, client: ClientId) {
        self.watch(client, false);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
