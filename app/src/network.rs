use crate::Daemon;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_network_api::Command;
use nova_socket::{Answer, Caller, done};
use std::sync::Arc;

use nova_network::{FEATURE_ID, Sources};

pub struct Network(Arc<nova_network::Network>);

impl std::ops::Deref for Network {
    type Target = nova_network::Network;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, caller: &Caller, command: &Command) -> Answer {
    let network = daemon.module::<Network>();
    let outcome = match command {
        Command::SetWifi { enabled } => {
            let enabled = *enabled;
            return later(network, move |network| done(network.set_wifi(enabled)));
        }
        Command::ScanWifi => return later(network, |network| done(network.scan())),
        Command::ConnectWifi { ssid, password } => {
            let (ssid, password) = (ssid.clone(), password.clone());
            return later(network, move |network| done(network.connect(&ssid, password.as_ref())));
        }
        Command::DisconnectWifi => return later(network, |network| done(network.disconnect())),
        Command::ForgetWifi { ssid } => {
            let ssid = ssid.clone();
            return later(network, move |network| done(network.forget(&ssid)));
        }
        Command::PreferNetwork { kind } => done(network.and_then(|network| network.prefer(*kind))),
        Command::WatchNetworkTraffic { watching } => {
            done(network.map(|network| network.watch_traffic(caller.id(), *watching)))
        }
    };
    Answer::Now(outcome)
}

fn later(network: Result<Arc<Network>, String>, work: impl FnOnce(&Network) -> Outcome + Send + 'static) -> Answer {
    match network {
        Ok(network) => Answer::later(move || work(&network)),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

impl nova_modules::Module for Network {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_network_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::system(&env.args.state_dir)
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(nova_network::Network::start(sources.clone(), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn client_gone(&self, client: ClientId) {
        self.watch_traffic(client, false);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
