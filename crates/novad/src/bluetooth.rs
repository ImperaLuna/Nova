//! Runs the Bluetooth feature (`nova-bluetooth`, in `features/bluetooth`)
//! in novad: its module and the answers to its commands. Wiring only: it
//! adapts the feature's core to novad's `Module` trait and `Daemon`, and
//! all of the feature's work and state stay in the core.

use crate::Daemon;
use nova_bluetooth_api::Command;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, done};
use std::sync::Arc;

pub use nova_bluetooth::{FEATURE_ID, Sources};
// What novad's own tests give it instead of the machine's.
#[cfg(test)]
pub use nova_bluetooth::{AUDIO_WAIT, CONNECT_LIMIT, Rfkill};

/// The feature's core, as novad's module.
pub struct Bluetooth(Arc<nova_bluetooth::Bluetooth>);

impl std::ops::Deref for Bluetooth {
    type Target = nova_bluetooth::Bluetooth;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands from `caller`, the route
/// `requests::router` gives them.
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

/// Runs `work` with Bluetooth on its own thread: BlueZ, the device and
/// the user's answer to pairing take their time.
fn later(
    bluetooth: Result<Arc<Bluetooth>, String>,
    work: impl FnOnce(&Bluetooth) -> Result<(), String> + Send + 'static,
) -> Answer {
    match bluetooth {
        Ok(bluetooth) => Answer::later(move || done(work(&bluetooth))),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

/// No options.
impl crate::modules::Module for Bluetooth {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_bluetooth_api::TOPIC);
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
        _: &[nova_protocol::FeatureState],
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
