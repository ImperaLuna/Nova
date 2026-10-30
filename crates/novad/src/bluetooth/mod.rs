//! Bluetooth (feature `bluetooth`): the radio, the devices and pairing, for
//! the quick settings tile and page, through BlueZ on the system bus.
//!
//! A thread follows BlueZ (`follow`): it reads every object BlueZ manages
//! once (`bluez::Objects`), then keeps them current from BlueZ's
//! `InterfacesAdded`, `InterfacesRemoved` and `PropertiesChanged`, and starts
//! over when bluetoothd stops or starts (`NameOwnerChanged`). It also serves
//! the pairing agent (`agent`) and registers it with BlueZ. Every change
//! publishes [`Event::Bluetooth`], when the event differs from the last one.
//!
//! Commands run on the caller's thread. Connecting a new device pairs it,
//! trusts it (so it reconnects by itself) and connects it, in one command;
//! the device's status says which step it's at, and a failure is kept on
//! it, in words for the user, until the next try. Turning Bluetooth on
//! lifts a software block first (`rfkill`).
//!
//! Scanning for new devices runs while any client watches
//! (`watch_bluetooth`, while the page shows), for at most [`SCAN_LIMIT`]
//! after the last start. A thread of its own (`scan`) starts and stops it.
//!
//! Tests give it a private bus: novad must never register as the pairing
//! agent of the machine running the tests.

mod agent;
mod bluez;
mod rfkill;
#[cfg(test)]
mod tests;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::session_bus::{Bus, Stop};
use agent::{AGENT_PATH, Agent, CAPABILITY, Reply, Slot};
use bluez::{ADAPTER, AGENT_MANAGER, BLUEZ, BLUEZ_PATH, DEVICE, Interfaces, Objects, Properties};
use futures_core::Stream;
use nova_protocol::{
    Bluetooth as BluetoothEvent, BluetoothRadio, BluetoothStatus, Event, PairingAnswer, PairingAsk, PairingRequest,
};
use std::collections::{BTreeSet, HashMap};
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::Poll;
use std::time::{Duration, Instant};
use zbus::wire::{ObjectPath, OwnedObjectPath, Variant};
use zbus::{Connection, MatchRule, MessageStream};

pub use rfkill::Rfkill;

pub const FEATURE_ID: &str = "bluetooth";
const LOG_SOURCE: &str = "bluetooth";
/// Device names are cut to this many characters.
const MAX_NAME_CHARS: usize = 100;
/// The most devices listed: more than are ever around.
const MAX_DEVICES: usize = 64;
/// How long a quick call to BlueZ may take (a property, scanning, forgetting).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long pairing and connecting may take: pairing waits for the user's
/// answer (up to [`agent::ANSWER_TIMEOUT`]), connecting for the device.
const LONG_CALL_TIMEOUT: Duration = Duration::from_secs(90);
/// How long scanning runs after the page asks for it.
const SCAN_LIMIT: Duration = Duration::from_secs(120);
/// After lifting a block, how long the adapter may take to power on, and how
/// often it's asked meanwhile. The kernel brings the adapter back up on its
/// own time; BlueZ refuses until it has.
const POWER_SETTLE: Duration = Duration::from_secs(3);
const POWER_RETRY: Duration = Duration::from_millis(100);
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

/// Where Bluetooth is: the machine's, or stand-ins in tests.
#[derive(Clone)]
pub struct Sources {
    /// The bus BlueZ is on.
    pub bus: Bus,
    pub rfkill: Rfkill,
}

impl Sources {
    pub fn system() -> Self {
        Self { bus: Bus::System, rfkill: Rfkill::system() }
    }
}

pub struct Bluetooth {
    shared: Arc<Shared>,
}

pub struct Shared {
    state: Mutex<State>,
    /// Wakes the scan thread when watchers, the radio or scanning change.
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    sources: Sources,
    stop: Stop,
}

#[derive(Default)]
struct State {
    objects: Objects,
    /// Quick calls to BlueZ, while it's followed.
    calls: Option<Connection>,
    /// The agent's connection, which pairs and connects (long calls).
    agent: Option<Connection>,
    /// What a command is doing to a device, by address.
    busy: HashMap<String, BluetoothStatus>,
    /// Why the last command on a device failed, by address.
    errors: HashMap<String, String>,
    /// The question pairing asks the user now.
    pairing: Option<Asked>,
    last_request: u64,
    watchers: BTreeSet<ClientId>,
    /// When a client last asked to scan.
    scan_asked: Option<Instant>,
    /// novad started scanning (BlueZ's `Discovering` says whether it runs).
    scanning: bool,
    sent: Option<BluetoothEvent>,
    stopped: bool,
}

/// A question to the user, and where its answer goes (none for a code
/// shown to type on the device).
struct Asked {
    request: PairingRequest,
    slot: Option<Arc<Slot>>,
}

impl Bluetooth {
    /// Starts following BlueZ and the scan thread. Returns at once.
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            hub,
            logger,
            sources,
            stop: Stop::default(),
        });
        // Clients that heard an earlier run (the feature was off a while)
        // start from what's known now: nothing yet.
        shared.publish(&mut lock(&shared.state));
        let follower = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(follow(&follower)) {
                follower.logger.info(LOG_SOURCE, &format!("not following BlueZ: {e}"));
            }
            follower.forget_bluez();
        });
        let scanner = Arc::clone(&shared);
        std::thread::spawn(move || scanner.scan());
        Arc::new(Self { shared })
    }

    /// Stops following BlueZ and scanning, and ends any question open.
    pub fn stop(&self) {
        let mut state = lock(&self.shared.state);
        state.stopped = true;
        if let Some(asked) = state.pairing.take() {
            asked.end(Reply::Cancelled);
        }
        drop(state);
        self.shared.stop.stop();
        self.shared.wake.notify_all();
    }

    /// Sends a new subscriber, client `id`, the state last published, under
    /// the lock publishing takes, so a newer event can't arrive before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        if let Some(sent) = &state.sent {
            self.shared.hub.send_to(id, &Event::Bluetooth(sent.clone()));
        }
    }

    /// Starts or stops `client` watching (scanning while any does).
    pub fn watch(&self, client: ClientId, watching: bool) {
        let mut state = lock(&self.shared.state);
        match watching {
            true => {
                state.watchers.insert(client);
                state.scan_asked = Some(Instant::now());
            }
            false => {
                state.watchers.remove(&client);
            }
        }
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Turns Bluetooth on (lifting a software block) or off.
    pub fn power(&self, on: bool) -> Result<(), String> {
        let shared = &self.shared;
        if on {
            let blocked = shared.sources.rfkill.blocked();
            if blocked.hard {
                return Err("Bluetooth is off by a switch or key on the computer: turn it on there.".into());
            }
            if blocked.soft {
                shared.sources.rfkill.unblock()?;
                shared.logger.info(LOG_SOURCE, "lifted the software block on Bluetooth");
            }
        }
        let deadline = Instant::now() + POWER_SETTLE;
        loop {
            let (calls, adapter) = {
                let state = lock(&shared.state);
                (state.calls.clone(), state.objects.adapter().map(|adapter| adapter.path))
            };
            let tried = match (calls, adapter) {
                (Some(calls), Some(adapter)) => zbus::block_on(set(&calls, &adapter, ADAPTER, "Powered", on)),
                _ => Err(zbus::Error::Failure("no Bluetooth adapter".into())),
            };
            match tried {
                Ok(()) => return Ok(()),
                // Just unblocked: the adapter comes back on its own time.
                Err(_) if on && Instant::now() < deadline => std::thread::sleep(POWER_RETRY),
                Err(e) => return Err(bluez::failure(&e).unwrap_or_else(|| e.to_string())),
            }
        }
    }

    /// Connects the device at `address`: pairs and trusts it first when
    /// it's new. Answers once it's connected, or why not.
    pub fn connect(&self, address: &str) -> Result<(), String> {
        let shared = &self.shared;
        let (path, paired, connected) = shared.device(address)?;
        if connected {
            return Ok(());
        }
        let result = (|| {
            if !paired {
                shared.set_busy(address, BluetoothStatus::Pairing);
                match shared.long_call(&path, DEVICE, "Pair") {
                    Err(e) if !named(&e, "AlreadyExists") => return Err(e),
                    _ => {}
                }
                zbus::block_on(set(&shared.calls()?, &path, DEVICE, "Trusted", true))?;
            }
            shared.set_busy(address, BluetoothStatus::Connecting);
            shared.long_call(&path, DEVICE, "Connect")
        })();
        shared.done(address, result)
    }

    pub fn disconnect(&self, address: &str) -> Result<(), String> {
        let shared = &self.shared;
        let (path, ..) = shared.device(address)?;
        shared.set_busy(address, BluetoothStatus::Disconnecting);
        let result = shared.long_call(&path, DEVICE, "Disconnect");
        shared.done(address, result)
    }

    /// Removes the device's pairing (BlueZ forgets it altogether).
    pub fn forget(&self, address: &str) -> Result<(), String> {
        let shared = &self.shared;
        let (path, ..) = shared.device(address)?;
        let adapter = lock(&shared.state).objects.adapter().map(|adapter| adapter.path);
        let Some(adapter) = adapter else { return Err("There's no Bluetooth adapter.".into()) };
        let result = shared.calls().and_then(|calls| {
            zbus::block_on(calls.call_method(Some(BLUEZ), adapter.as_str(), Some(ADAPTER), "RemoveDevice", &(path,)))
                .map(|_| ())
        });
        lock(&shared.state).errors.remove(&address.to_uppercase());
        result.map_err(|e| bluez::failure(&e).unwrap_or_else(|| e.to_string()))
    }

    /// The user's answer to pairing question `id`.
    pub fn answer(&self, id: u64, answer: PairingAnswer) -> Result<(), String> {
        let shared = &self.shared;
        let mut state = lock(&shared.state);
        let Some(asked) = state.pairing.as_ref().filter(|asked| asked.request.id == id) else {
            return Err("that pairing question is over".into());
        };
        agent::fits(&asked.request.ask, &answer)?;
        match &asked.slot {
            Some(slot) => slot.resolve(Reply::Answered(answer)),
            // A code to type on the device: turning it down stops pairing.
            None => {
                let address = asked.request.address.clone();
                state.pairing = None;
                shared.publish(&mut state);
                let path = state.objects.device(&address).map(|device| device.path);
                drop(state);
                if let Some(path) = path {
                    let cancelled =
                        shared.calls().and_then(|calls| zbus::block_on(call(&calls, &path, DEVICE, "CancelPairing")));
                    if let Err(e) = cancelled {
                        shared.logger.info(LOG_SOURCE, &format!("can't cancel pairing with {address}: {e}"));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Shared {
    /// The event for `state`, published when it differs from the last one.
    fn publish(&self, state: &mut MutexGuard<'_, State>) {
        let event = self.event(state);
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Bluetooth(event.clone()));
            state.sent = Some(event);
        }
        self.wake.notify_all();
    }

    fn event(&self, state: &State) -> BluetoothEvent {
        let blocked = self.sources.rfkill.blocked();
        let adapter = state.objects.adapter();
        let radio = match &adapter {
            None => BluetoothRadio::Missing,
            Some(_) if blocked.hard => BluetoothRadio::SwitchedOff,
            Some(adapter) if adapter.powered && !blocked.soft => BluetoothRadio::On,
            Some(_) => BluetoothRadio::Off,
        };
        let on = radio == BluetoothRadio::On;
        let mut devices: Vec<_> = match on {
            true => state
                .objects
                .devices()
                .iter()
                .filter(|device| device.listed())
                .map(|device| {
                    let key = device.address.to_uppercase();
                    device.event(state.busy.get(&key).copied(), state.errors.get(&key).cloned())
                })
                .collect(),
            false => Vec::new(),
        };
        devices.sort_by_cached_key(bluez::order);
        devices.truncate(MAX_DEVICES);
        BluetoothEvent {
            radio,
            scanning: on && adapter.is_some_and(|adapter| adapter.discovering),
            devices,
            pairing: state.pairing.as_ref().map(|asked| asked.request.clone()),
        }
    }

    /// The device at `address`: its path, and whether it's paired and
    /// connected.
    fn device(&self, address: &str) -> Result<(OwnedObjectPath, bool, bool), String> {
        if !bluez::is_address(address) {
            return Err(format!("{address:?} isn't a Bluetooth address"));
        }
        let state = lock(&self.state);
        let device = state.objects.device(address).ok_or("That device isn't around any more.")?;
        Ok((device.path, device.paired, device.connected))
    }

    fn set_busy(&self, address: &str, status: BluetoothStatus) {
        let mut state = lock(&self.state);
        let key = address.to_uppercase();
        state.errors.remove(&key);
        state.busy.insert(key, status);
        self.publish(&mut state);
    }

    /// A command on `address` ended with `result`: its failure is kept on
    /// the device, and a code shown for it goes.
    fn done(&self, address: &str, result: zbus::Result<()>) -> Result<(), String> {
        let mut state = lock(&self.state);
        let key = address.to_uppercase();
        state.busy.remove(&key);
        let shown = state.pairing.as_ref().is_some_and(|asked| asked.slot.is_none() && asked.request.address == key);
        if shown {
            state.pairing = None;
        }
        let outcome = match result {
            Ok(()) => Ok(()),
            Err(e) => {
                self.logger.info(LOG_SOURCE, &format!("{address}: {e}"));
                match bluez::failure(&e) {
                    Some(words) => {
                        state.errors.insert(key, words.clone());
                        Err(words)
                    }
                    // The user cancelled: nothing to say.
                    None => Ok(()),
                }
            }
        };
        self.publish(&mut state);
        outcome
    }

    /// Calls `method` of `interface` on `path` over the agent's connection,
    /// so BlueZ asks novad's agent.
    fn long_call(&self, path: &OwnedObjectPath, interface: &'static str, method: &'static str) -> zbus::Result<()> {
        let agent = lock(&self.state).agent.clone().ok_or_else(not_followed)?;
        zbus::block_on(call(&agent, path, interface, method))
    }

    /// The connection for quick calls.
    fn calls(&self) -> zbus::Result<Connection> {
        lock(&self.state).calls.clone().ok_or_else(not_followed)
    }

    /// Puts `ask` to the user about the device at `path`. `wait`: the agent
    /// waits for the answer in the returned slot, until
    /// [`agent::ANSWER_TIMEOUT`]. `None` for a device BlueZ hasn't listed.
    fn ask(&self, path: &str, ask: PairingAsk, wait: bool) -> Option<(u64, Option<Arc<Slot>>)> {
        let mut state = lock(&self.state);
        let address = state.objects.address(path)?.to_uppercase();
        // BlueZ asks one thing at a time: an older question is over.
        if let Some(older) = state.pairing.take() {
            older.end(Reply::Cancelled);
        }
        state.last_request += 1;
        let id = state.last_request;
        let incoming = state.busy.get(&address) != Some(&BluetoothStatus::Pairing);
        let slot = wait.then(|| Arc::new(Slot::default()));
        state.pairing = Some(Asked { request: PairingRequest { id, address, ask, incoming }, slot: slot.clone() });
        self.publish(&mut state);
        if let Some(slot) = &slot {
            let slot = Arc::clone(slot);
            std::thread::spawn(move || {
                std::thread::sleep(agent::ANSWER_TIMEOUT);
                slot.resolve(Reply::TimedOut);
            });
        }
        Some((id, slot))
    }

    /// Question `id` has its reply: it's no longer shown.
    fn asked(&self, id: u64) {
        let mut state = lock(&self.state);
        if state.pairing.as_ref().is_some_and(|asked| asked.request.id == id) {
            state.pairing = None;
            self.publish(&mut state);
        }
    }

    /// BlueZ cancelled pairing.
    fn cancel_pairing(&self) {
        let mut state = lock(&self.state);
        if let Some(asked) = state.pairing.take() {
            asked.end(Reply::Cancelled);
            self.publish(&mut state);
        }
    }

    /// BlueZ went, or isn't followed any more.
    fn forget_bluez(&self) {
        let mut state = lock(&self.state);
        state.objects.clear();
        state.calls = None;
        state.agent = None;
        state.scanning = false;
        if let Some(asked) = state.pairing.take() {
            asked.end(Reply::Cancelled);
        }
        if !state.stopped {
            self.publish(&mut state);
        }
    }

    /// Starts and stops scanning, until stopped.
    fn scan(&self) {
        let mut state = lock(&self.state);
        // BlueZ has said it's scanning since novad started it.
        let mut confirmed = false;
        loop {
            if state.stopped {
                return;
            }
            let now = Instant::now();
            let until = state.scan_asked.map(|asked| asked + SCAN_LIMIT).filter(|until| *until > now);
            let adapter = state.objects.adapter().map(|adapter| (adapter.path, adapter.powered, adapter.discovering));
            let want = !state.watchers.is_empty() && until.is_some() && adapter.as_ref().is_some_and(|a| a.1);
            let discovering = adapter.as_ref().is_some_and(|a| a.2);
            // Ended by BlueZ (powered off, adapter gone): it's off.
            if state.scanning && confirmed && !discovering {
                state.scanning = false;
            }
            confirmed = state.scanning && (confirmed || discovering);
            let running = state.scanning;
            match (want, running, adapter, state.calls.clone()) {
                (true, false, Some((path, ..)), Some(calls)) => {
                    state.scanning = true;
                    drop(state);
                    if let Err(e) = zbus::block_on(call(&calls, &path, ADAPTER, "StartDiscovery")) {
                        self.logger.info(LOG_SOURCE, &format!("can't scan for devices: {e}"));
                    }
                    state = lock(&self.state);
                }
                (false, _, Some((path, ..)), Some(calls)) if state.scanning => {
                    state.scanning = false;
                    drop(state);
                    // BlueZ also stops it when our connection goes.
                    if let Err(e) = zbus::block_on(call(&calls, &path, ADAPTER, "StopDiscovery")) {
                        self.logger.log(nova_protocol::LogLevel::Debug, LOG_SOURCE, &format!("stop scanning: {e}"));
                    }
                    state = lock(&self.state);
                }
                _ => {
                    state = match until {
                        Some(until) => {
                            self.wake
                                .wait_timeout(state, until - now)
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .0
                        }
                        None => self.wake.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner),
                    };
                }
            }
        }
    }
}

impl Asked {
    fn end(self, reply: Reply) {
        if let Some(slot) = self.slot {
            slot.resolve(reply);
        }
    }
}

fn not_followed() -> zbus::Error {
    zbus::Error::Failure("BlueZ isn't running".into())
}

/// Whether `error` is BlueZ's error `name` (`org.bluez.Error.<name>`).
fn named(error: &zbus::Error, name: &str) -> bool {
    matches!(error, zbus::Error::MethodError(error, ..) if error.as_str().strip_prefix("org.bluez.Error.") == Some(name))
}

async fn call(connection: &Connection, path: &OwnedObjectPath, interface: &str, method: &str) -> zbus::Result<()> {
    connection.call_method(Some(BLUEZ), path.as_str(), Some(interface), method, &()).await.map(|_| ())
}

async fn set(
    connection: &Connection,
    path: &OwnedObjectPath,
    interface: &str,
    name: &str,
    value: bool,
) -> zbus::Result<()> {
    let value = Variant::from(value);
    connection
        .call_method(Some(BLUEZ), path.as_str(), Some(PROPERTIES), "Set", &(interface, name, value))
        .await
        .map(|_| ())
}

/// Follows BlueZ on the bus until stopped or the bus goes.
async fn follow(shared: &Arc<Shared>) -> zbus::Result<()> {
    let calls = shared.sources.bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
    // The agent's object server listens before BlueZ can call it: the
    // builder serves it from the start.
    let agent = Agent { shared: Arc::clone(shared) };
    let agent =
        shared.sources.bus.builder().method_timeout(LONG_CALL_TIMEOUT).serve_at(AGENT_PATH, agent).build().await?;
    let signal = || MatchRule::builder().msg_type(zbus::message::Type::Signal);
    // Followed before anything is read, so no change in between is missed.
    let owner_rule =
        signal().sender(DBUS_NAME).interface(DBUS_NAME).member("NameOwnerChanged").arg(0, BLUEZ).build()?;
    let mut owners = MessageStream::for_match_rule(owner_rule, &calls, None).await?;
    let objects_rule = signal().interface(OBJECT_MANAGER).path("/").build()?;
    let mut comings = MessageStream::for_match_rule(objects_rule, &calls, None).await?;
    let changed_rule = signal().interface(PROPERTIES).member("PropertiesChanged").path_namespace(BLUEZ_PATH).build()?;
    let mut changes = MessageStream::for_match_rule(changed_rule, &calls, None).await?;
    {
        let mut state = lock(&shared.state);
        state.calls = Some(calls.clone());
        state.agent = Some(agent.clone());
    }
    // bluetoothd's unique name while it runs: only its signals count.
    let mut owner = name_owner(&calls).await;
    match &owner {
        Some(_) => start_over(shared, &calls, &agent).await,
        None => shared.logger.info(LOG_SOURCE, "BlueZ isn't running; following it in case it starts"),
    }
    loop {
        let next = poll_fn(|cx| {
            if shared.stop.poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            for stream in [&mut owners, &mut comings, &mut changes] {
                if let Poll::Ready(message) = Pin::new(stream).poll_next(cx) {
                    return Poll::Ready(message);
                }
            }
            Poll::Pending
        })
        .await;
        let Some(message) = next else {
            unregister(&agent).await;
            return Ok(());
        };
        let message = message?;
        let header = message.header();
        let member = header.member().map(|member| member.to_string()).unwrap_or_default();
        if member == "NameOwnerChanged" {
            let Ok((_, _, new)) = message.body().deserialize::<(String, String, String)>() else { continue };
            owner = (!new.is_empty()).then_some(new);
            match owner {
                Some(_) => {
                    shared.logger.info(LOG_SOURCE, "BlueZ started");
                    start_over(shared, &calls, &agent).await;
                }
                None => {
                    shared.logger.info(LOG_SOURCE, "BlueZ stopped");
                    let mut state = lock(&shared.state);
                    state.objects.clear();
                    state.scanning = false;
                    shared.publish(&mut state);
                }
            }
            continue;
        }
        let from_bluez = owner.is_some() && header.sender().map(|sender| sender.to_string()) == owner;
        if !from_bluez {
            continue;
        }
        let mut state = lock(&shared.state);
        match member.as_str() {
            "InterfacesAdded" => {
                let Ok((path, interfaces)) = message.body().deserialize::<(OwnedObjectPath, Interfaces)>() else {
                    continue;
                };
                state.objects.added(path, interfaces);
            }
            "InterfacesRemoved" => {
                let Ok((path, interfaces)) = message.body().deserialize::<(OwnedObjectPath, Vec<String>)>() else {
                    continue;
                };
                state.objects.removed(&path, &interfaces);
            }
            "PropertiesChanged" => {
                let Some(path) = header.path().map(|path| OwnedObjectPath::from(path.to_owned())) else { continue };
                let Ok((interface, changed, invalidated)) =
                    message.body().deserialize::<(String, Properties, Vec<String>)>()
                else {
                    continue;
                };
                state.objects.changed(&path, &interface, changed, &invalidated);
            }
            _ => continue,
        }
        shared.publish(&mut state);
    }
}

/// Registers the agent with BlueZ, then reads everything BlueZ has: once
/// the devices show, connecting one pairs through the agent.
async fn start_over(shared: &Shared, calls: &Connection, agent: &Connection) {
    match register(agent).await {
        Ok(()) => shared.logger.info(LOG_SOURCE, "pairing agent registered"),
        Err(e) => shared.logger.warn(LOG_SOURCE, &format!("can't register the pairing agent: {e}")),
    }
    let objects = calls
        .call_method(Some(BLUEZ), "/", Some(OBJECT_MANAGER), "GetManagedObjects", &())
        .await
        .and_then(|reply| reply.body().deserialize::<HashMap<OwnedObjectPath, Interfaces>>());
    match objects {
        Ok(objects) => {
            let mut state = lock(&shared.state);
            state.objects.replace(objects);
            shared.publish(&mut state);
        }
        Err(e) => shared.logger.warn(LOG_SOURCE, &format!("can't read BlueZ's adapters and devices: {e}")),
    }
}

async fn register(agent: &Connection) -> zbus::Result<()> {
    let path = ObjectPath::try_from(AGENT_PATH)?;
    agent.call_method(Some(BLUEZ), BLUEZ_PATH, Some(AGENT_MANAGER), "RegisterAgent", &(&path, CAPABILITY)).await?;
    agent.call_method(Some(BLUEZ), BLUEZ_PATH, Some(AGENT_MANAGER), "RequestDefaultAgent", &(&path,)).await?;
    Ok(())
}

/// Takes the agent back from BlueZ. BlueZ drops it anyway when the
/// connection goes; this says so at once.
async fn unregister(agent: &Connection) {
    if let Ok(path) = ObjectPath::try_from(AGENT_PATH) {
        let _ = agent.call_method(Some(BLUEZ), BLUEZ_PATH, Some(AGENT_MANAGER), "UnregisterAgent", &(&path,)).await;
    }
}

/// bluetoothd's unique name, `None` while it doesn't run.
async fn name_owner(connection: &Connection) -> Option<String> {
    let reply =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(BLUEZ,)).await.ok()?;
    reply.body().deserialize().ok()
}
