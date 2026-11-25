mod agent;
mod bluez;
mod connecting;
mod rfkill;
#[cfg(test)]
mod tests;

use agent::{AGENT_PATH, Agent, CAPABILITY, Reply, Slot};
use bluez::{ADAPTER, AGENT_MANAGER, BLUEZ, BLUEZ_PATH, DEVICE, Interfaces, Objects, Properties};
use futures_core::Stream;
use nova_base::lock;
use nova_bluetooth_api::{
    Bluetooth as BluetoothEvent, BluetoothRadio, BluetoothStatus, Event, PairingAnswer, PairingAsk, PairingRequest,
};
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_session_bus::{Bus, Stop};
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
const MAX_NAME_CHARS: usize = 100;
const MAX_DEVICES: usize = 64;
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const LONG_CALL_TIMEOUT: Duration = Duration::from_secs(90);
pub const CONNECT_LIMIT: Duration = Duration::from_secs(30);
const SCAN_LIMIT: Duration = Duration::from_secs(120);
const SCAN_RETRY: Duration = Duration::from_secs(2);
const POWER_SETTLE: Duration = Duration::from_secs(3);
const POWER_RETRY: Duration = Duration::from_millis(100);
pub const AUDIO_WAIT: Duration = Duration::from_secs(4);
const AUDIO_RETRIES: u32 = 2;
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    pub rfkill: Rfkill,
    pub connect_limit: Duration,
    pub audio_wait: Duration,
}

impl Sources {
    pub fn system() -> Self {
        Self { bus: Bus::System, rfkill: Rfkill::system(), connect_limit: CONNECT_LIMIT, audio_wait: AUDIO_WAIT }
    }
}

pub struct Bluetooth {
    shared: Arc<Shared>,
}

pub struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    sources: Sources,
    stop: Stop,
}

#[derive(Default)]
struct State {
    objects: Objects,
    calls: Option<Connection>,
    agent: Option<Connection>,
    busy: HashMap<String, BluetoothStatus>,
    errors: HashMap<String, String>,
    halted: BTreeSet<String>,
    connects: HashMap<String, u64>,
    last_connect: u64,
    timed_out: BTreeSet<String>,
    pairing: Option<Asked>,
    last_request: u64,
    watchers: BTreeSet<ClientId>,
    scan_asked: Option<Instant>,
    scanning: bool,
    sent: Option<BluetoothEvent>,
    stopped: bool,
}

struct Asked {
    request: PairingRequest,
    slot: Option<Arc<Slot>>,
}

impl Bluetooth {
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            hub,
            logger,
            sources,
            stop: Stop::default(),
        });
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

    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        if let Some(sent) = &state.sent {
            self.shared.hub.send_to(id, &Event::Bluetooth(sent.clone()));
        }
    }

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

    pub fn power(&self, on: bool) -> Result<(), String> {
        let shared = &self.shared;
        if on {
            let adapter = lock(&shared.state).objects.adapter().map(|adapter| adapter.path);
            let name = adapter.as_ref().map(bluez::adapter_name);
            let blocked = shared.sources.rfkill.blocked(name);
            if blocked.hard {
                return Err("Blocked by a hardware switch".into());
            }
            if blocked.soft {
                shared.sources.rfkill.unblock(name)?;
                shared.logger.info(LOG_SOURCE, "lifted the software block on Bluetooth");
            }
        }
        if !on {
            let busy: Vec<_> = {
                let state = lock(&shared.state);
                state
                    .busy
                    .iter()
                    .filter_map(|(address, status)| Some((state.objects.device(address)?.path, Some(*status))))
                    .collect()
            };
            for (path, status) in busy {
                shared.cancel(&path, status);
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
                Err(e) if (on || named(&e, "Busy")) && Instant::now() < deadline => {
                    nova_base::backoff::wait_out(POWER_RETRY)
                }
                Err(e) => return Err(bluez::failure(&e).unwrap_or_else(|| e.to_string())),
            }
        }
    }

    pub fn connect(&self, address: &str) -> Result<(), String> {
        let shared = &self.shared;
        let (path, paired, connected) = shared.device(address)?;
        if connected {
            return Ok(());
        }
        lock(&shared.state).halted.remove(&address.to_uppercase());
        let result = (|| {
            if !paired {
                shared.set_busy(address, BluetoothStatus::Pairing);
                match shared.long_call(&path, DEVICE, "Pair") {
                    Err(e) if !named(&e, "AlreadyExists") => return Err(e),
                    _ => {}
                }
                shared.go_on(address)?;
                zbus::block_on(set(&shared.calls()?, &path, DEVICE, "Trusted", true))?;
            }
            shared.set_busy(address, BluetoothStatus::Connecting);
            shared.go_on(address)?;
            Shared::limit_connect(shared, address, &path);
            match shared.long_call(&path, DEVICE, "Connect") {
                Err(e) if bluez::le_aborted(&e) => shared.connect_classic(address, &path, e),
                result => result,
            }?;
            shared.ensure_a2dp(address, &path);
            Ok(())
        })();
        shared.done(address, result)
    }

    pub fn disconnect(&self, address: &str) -> Result<(), String> {
        let shared = &self.shared;
        let (path, ..) = shared.device(address)?;
        let busy = lock(&shared.state).busy.get(&address.to_uppercase()).copied();
        match busy {
            Some(BluetoothStatus::Pairing | BluetoothStatus::Connecting) => {
                lock(&shared.state).halted.insert(address.to_uppercase());
                shared.cancel(&path, busy);
                Ok(())
            }
            _ => {
                shared.set_busy(address, BluetoothStatus::Disconnecting);
                let result = shared.long_call(&path, DEVICE, "Disconnect");
                shared.done(address, result)
            }
        }
    }

    pub fn forget(&self, address: &str) -> Result<(), String> {
        let shared = &self.shared;
        let (path, ..) = shared.device(address)?;
        let adapter = lock(&shared.state).objects.adapter().map(|adapter| adapter.path);
        let Some(adapter) = adapter else { return Err("No Bluetooth adapter".into()) };
        let result = shared.calls().and_then(|calls| {
            zbus::block_on(calls.call_method(Some(BLUEZ), adapter.as_str(), Some(ADAPTER), "RemoveDevice", &(path,)))
                .map(|_| ())
        });
        lock(&shared.state).errors.remove(&address.to_uppercase());
        result.map_err(|e| bluez::failure(&e).unwrap_or_else(|| e.to_string()))
    }

    pub fn answer(&self, id: u64, answer: PairingAnswer) -> Result<(), String> {
        let shared = &self.shared;
        let mut state = lock(&shared.state);
        let Some(asked) = state.pairing.as_ref().filter(|asked| asked.request.id == id) else {
            return Err("that pairing question is over".into());
        };
        agent::fits(&asked.request.ask, &answer)?;
        match &asked.slot {
            Some(slot) => slot.resolve(Reply::Answered(answer)),
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
    fn publish(&self, state: &mut MutexGuard<'_, State>) {
        if state.stopped {
            self.wake.notify_all();
            return;
        }
        let event = self.event(state);
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Bluetooth(event.clone()));
            state.sent = Some(event);
        }
        self.wake.notify_all();
    }

    fn event(&self, state: &State) -> BluetoothEvent {
        let adapter = state.objects.adapter();
        let blocked = self.sources.rfkill.blocked(adapter.as_ref().map(|adapter| bluez::adapter_name(&adapter.path)));
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

    fn long_call(&self, path: &OwnedObjectPath, interface: &'static str, method: &'static str) -> zbus::Result<()> {
        let agent = lock(&self.state).agent.clone().ok_or_else(not_followed)?;
        zbus::block_on(call(&agent, path, interface, method))
    }

    fn calls(&self) -> zbus::Result<Connection> {
        lock(&self.state).calls.clone().ok_or_else(not_followed)
    }

    fn ask(&self, path: &str, ask: PairingAsk) -> Option<(u64, Arc<Slot>)> {
        let slot = Arc::new(Slot::default());
        let id = self.pose(path, ask, Some(Arc::clone(&slot)))?;
        let timeout = Arc::clone(&slot);
        std::thread::spawn(move || {
            if timeout.wait_for(agent::ANSWER_TIMEOUT).is_none() {
                timeout.resolve(Reply::TimedOut);
            }
        });
        Some((id, slot))
    }

    fn show(&self, path: &str, ask: PairingAsk) {
        self.pose(path, ask, None);
    }

    fn pose(&self, path: &str, ask: PairingAsk, slot: Option<Arc<Slot>>) -> Option<u64> {
        let mut state = lock(&self.state);
        let address = state.objects.address(path)?.to_uppercase();
        if let Some(older) = state.pairing.take() {
            older.end(Reply::Cancelled);
        }
        state.last_request += 1;
        let id = state.last_request;
        let incoming = state.busy.get(&address) != Some(&BluetoothStatus::Pairing);
        state.pairing = Some(Asked { request: PairingRequest { id, address, ask, incoming }, slot });
        self.publish(&mut state);
        Some(id)
    }

    fn asked(&self, id: u64) {
        let mut state = lock(&self.state);
        if state.pairing.as_ref().is_some_and(|asked| asked.request.id == id) {
            state.pairing = None;
            self.publish(&mut state);
        }
    }

    fn cancel_pairing(&self) {
        let mut state = lock(&self.state);
        if let Some(asked) = state.pairing.take() {
            asked.end(Reply::Cancelled);
            self.publish(&mut state);
        }
    }

    fn forget_bluez(&self) {
        let mut state = lock(&self.state);
        state.objects.clear();
        state.calls = None;
        state.agent = None;
        state.scanning = false;
        if let Some(asked) = state.pairing.take() {
            asked.end(Reply::Cancelled);
        }
        self.publish(&mut state);
    }

    fn scan(&self) {
        let mut state = lock(&self.state);
        let mut confirmed = false;
        let mut retry_at: Option<Instant> = None;
        loop {
            if state.stopped {
                return;
            }
            let now = Instant::now();
            let until = state.scan_asked.map(|asked| asked + SCAN_LIMIT).filter(|until| *until > now);
            let adapter = state.objects.adapter().map(|adapter| (adapter.path, adapter.powered, adapter.discovering));
            let resting = retry_at.filter(|at| *at > now);
            let connecting = state.busy.values().any(|status| *status == BluetoothStatus::Connecting);
            let want = !state.watchers.is_empty()
                && until.is_some()
                && resting.is_none()
                && !connecting
                && adapter.as_ref().is_some_and(|a| a.1);
            let discovering = adapter.as_ref().is_some_and(|a| a.2);
            if state.scanning && confirmed && !discovering {
                state.scanning = false;
            }
            confirmed = state.scanning && (confirmed || discovering);
            let running = state.scanning;
            match (want, running, adapter, state.calls.clone()) {
                (true, false, Some((path, ..)), Some(calls)) => {
                    state.scanning = true;
                    drop(state);
                    let started = zbus::block_on(call(&calls, &path, ADAPTER, "StartDiscovery"));
                    state = lock(&self.state);
                    if let Err(e) = started {
                        self.logger.info(LOG_SOURCE, &format!("can't scan for devices: {e}"));
                        state.scanning = false;
                        retry_at = Some(Instant::now() + SCAN_RETRY);
                    }
                }
                (false, _, Some((path, ..)), Some(calls)) if state.scanning => {
                    state.scanning = false;
                    drop(state);
                    if let Err(e) = zbus::block_on(call(&calls, &path, ADAPTER, "StopDiscovery")) {
                        self.logger.log(nova_carrier::log::LogLevel::Debug, LOG_SOURCE, &format!("stop scanning: {e}"));
                    }
                    state = lock(&self.state);
                }
                _ => {
                    let until = match (until, resting) {
                        (Some(until), Some(resting)) => Some(until.min(resting)),
                        (until, _) => until,
                    };
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

async fn follow(shared: &Arc<Shared>) -> zbus::Result<()> {
    let calls = shared.sources.bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
    let agent = Agent { shared: Arc::clone(shared) };
    let agent =
        shared.sources.bus.builder().method_timeout(LONG_CALL_TIMEOUT).serve_at(AGENT_PATH, agent).build().await?;
    let signal = || MatchRule::builder().msg_type(zbus::message::Type::Signal);
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
        if apply(&mut state.objects, &member, &message) {
            shared.publish(&mut state);
        }
    }
}

fn apply(objects: &mut Objects, member: &str, message: &zbus::Message) -> bool {
    match member {
        "InterfacesAdded" => {
            let Ok((path, interfaces)) = message.body().deserialize::<(OwnedObjectPath, Interfaces)>() else {
                return false;
            };
            objects.added(path, interfaces);
        }
        "InterfacesRemoved" => {
            let Ok((path, interfaces)) = message.body().deserialize::<(OwnedObjectPath, Vec<String>)>() else {
                return false;
            };
            objects.removed(&path, &interfaces);
        }
        "PropertiesChanged" => {
            let header = message.header();
            let Some(path) = header.path().map(|path| OwnedObjectPath::from(path.to_owned())) else { return false };
            let Ok((interface, changed, invalidated)) =
                message.body().deserialize::<(String, Properties, Vec<String>)>()
            else {
                return false;
            };
            objects.changed(&path, &interface, changed, &invalidated);
        }
        _ => return false,
    }
    true
}

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

async fn unregister(agent: &Connection) {
    if let Ok(path) = ObjectPath::try_from(AGENT_PATH) {
        let _ = agent.call_method(Some(BLUEZ), BLUEZ_PATH, Some(AGENT_MANAGER), "UnregisterAgent", &(&path,)).await;
    }
}

async fn name_owner(connection: &Connection) -> Option<String> {
    let reply =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(BLUEZ,)).await.ok()?;
    reply.body().deserialize().ok()
}
