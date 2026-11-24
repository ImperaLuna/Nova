use crate::Daemon;
use nova_bluetooth_api::Command;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, done};
use std::sync::Arc;

use nova_bluetooth::{FEATURE_ID, Sources};

pub struct Bluetooth(Arc<nova_bluetooth::Bluetooth>);

impl std::ops::Deref for Bluetooth {
    type Target = nova_bluetooth::Bluetooth;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, caller: &Caller, command: &Command) -> Answer {
    let bluetooth = daemon.module::<Bluetooth>();
    let outcome = match command {
        Command::WatchBluetooth { watching } => {
            done(bluetooth.map(|bluetooth| bluetooth.watch(caller.id(), *watching)))
        }
        Command::BluetoothPower { on } => {
            let on = *on;
            return later(bluetooth, move |bluetooth| bluetooth.power(on));
        }
        Command::BluetoothConnect { address } => {
            let address = address.clone();
            return later(bluetooth, move |bluetooth| bluetooth.connect(&address));
        }
        Command::BluetoothDisconnect { address } => {
            let address = address.clone();
            return later(bluetooth, move |bluetooth| bluetooth.disconnect(&address));
        }
        Command::BluetoothForget { address } => {
            let address = address.clone();
            return later(bluetooth, move |bluetooth| bluetooth.forget(&address));
        }
        Command::BluetoothAnswer { id, answer } => {
            done(bluetooth.and_then(|bluetooth| bluetooth.answer(*id, answer.clone())))
        }
    };
    Answer::Now(outcome)
}

fn later(
    bluetooth: Result<Arc<Bluetooth>, String>,
    work: impl FnOnce(&Bluetooth) -> Result<(), String> + Send + 'static,
) -> Answer {
    match bluetooth {
        Ok(bluetooth) => Answer::later(move || done(work(&bluetooth))),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

impl nova_modules::Module for Bluetooth {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_bluetooth_api::TOPIC);
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
        Arc::new(Self(nova_bluetooth::Bluetooth::start(sources.clone(), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn client_gone(&self, client: ClientId) {
        self.watch(client, false);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
