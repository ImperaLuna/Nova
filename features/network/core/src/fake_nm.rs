//! A stand-in NetworkManager on a test's bus, serving the objects of a
//! recording (`recorded/nm-*.json`: every object, with the properties
//! novad reads as `busctl --json=short` prints them, and each saved
//! profile's `GetSettings`). It answers calls itself, straight off the bus, rather
//! than through zbus's object server: one generic handler serves whatever
//! the recording holds.
//!
//! It acts like NetworkManager where novad's calls change something:
//! Wi-Fi's switch, scans, activating a saved or a new profile (which
//! connects when its password is the network's, else fails for missing
//! secrets), deactivating, deleting profiles, and reapplying route
//! metrics (the link with the lowest becomes the primary connection). It
//! says so with `PropertiesChanged` and the active connection's
//! `StateChanged`, as NetworkManager does.

use super::manager::{
    ACCESS_POINT, ACTIVE, DEVICE, NM, NM_PATH, NO_OBJECT, PROPERTIES, SETTINGS, SETTINGS_CONNECTION, WIRELESS,
};
use futures_core::Stream;
use nova_base::lock;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use zbus::message::Type;
use zbus::wire::{OwnedObjectPath, OwnedVariant, Structure, Variant};
use zbus::{Connection, Message, MessageStream};

/// What `connect` and `PropertiesChanged` carry: (signature, JSON value).
type Property = (String, Value);
type Interfaces = BTreeMap<String, BTreeMap<String, Property>>;
type Settings = HashMap<String, HashMap<String, OwnedVariant>>;

const ACTIVE_PREFIX: &str = "/org/freedesktop/NetworkManager/ActiveConnection/";
const SETTINGS_PREFIX: &str = "/org/freedesktop/NetworkManager/Settings/";
/// `NMActiveConnectionState`s and the reason for a missing secret.
const ACTIVATING: u32 = 1;
const ACTIVATED: u32 = 2;
const DEACTIVATED: u32 = 4;
const REASON_NONE: u32 = 0;
const REASON_USER: u32 = 2;
const REASON_NO_SECRETS: u32 = 9;
const DEVICE_ACTIVATED: u32 = 100;
const DEVICE_DISCONNECTED: u32 = 30;

pub struct FakeNm {
    connection: Connection,
    state: Arc<Mutex<Fake>>,
}

#[derive(Default)]
struct Fake {
    objects: BTreeMap<String, Interfaces>,
    /// Each profile's settings (JSON as recorded), by path.
    profiles: BTreeMap<String, Value>,
    /// The password each network takes; a network that isn't here takes
    /// whatever its saved profile has.
    passwords: HashMap<String, String>,
    /// Passwords saved in profiles, by profile path.
    saved_passwords: HashMap<String, String>,
    /// Route metrics (IPv4, IPv6) reapplied on devices, by device path.
    metrics: HashMap<String, [i64; 2]>,
    next_id: u32,
    /// A method whose next call fails, as NetworkManager's do when a
    /// connection changed under the caller.
    fail_next: Option<String>,
    /// Activations stay activating, as when the network doesn't answer.
    stall: bool,
    /// Every call novad made: (path, interface, method).
    calls: Vec<(String, String, String)>,
}

impl FakeNm {
    /// Serves `recording` on `connection` under NetworkManager's name.
    pub fn start(connection: Connection, recording: &Value) -> Self {
        let mut fake = Fake { next_id: 100, ..Fake::default() };
        for (path, interfaces) in recording.as_object().unwrap() {
            for (interface, content) in interfaces.as_object().unwrap() {
                if interface == SETTINGS_CONNECTION {
                    fake.profiles.insert(path.clone(), content["GetSettings"]["data"][0].clone());
                    continue;
                }
                let properties = content["data"][0].as_object().unwrap();
                let properties = properties
                    .iter()
                    .map(|(name, value)| {
                        (name.clone(), (value["type"].as_str().unwrap().to_owned(), value["data"].clone()))
                    })
                    .collect();
                fake.objects.entry(path.clone()).or_default().insert(interface.clone(), properties);
            }
        }
        let state = Arc::new(Mutex::new(fake));
        let serving = Self { connection: connection.clone(), state: Arc::clone(&state) };
        let answering = Self { connection: connection.clone(), state };
        std::thread::spawn(move || zbus::block_on(answering.answer()));
        zbus::block_on(connection.request_name(NM)).unwrap();
        serving
    }

    /// The password `ssid` takes.
    pub fn set_password(&self, ssid: &str, password: &str) {
        lock(&self.state).passwords.insert(ssid.into(), password.into());
    }

    /// The calls novad made of `method`, by path.
    pub fn calls(&self, method: &str) -> Vec<String> {
        lock(&self.state).calls.iter().filter(|call| call.2 == method).map(|call| call.0.clone()).collect()
    }

    /// Leaves connections it activates from now activating.
    pub fn stall(&self) {
        lock(&self.state).stall = true;
    }

    /// Goes off the bus, as NetworkManager does when it stops.
    pub fn vanish(&self) {
        zbus::block_on(self.connection.release_name(NM)).unwrap();
    }

    /// Makes the next call of `method` fail.
    pub fn fail_next(&self, method: &str) {
        lock(&self.state).fail_next = Some(method.to_owned());
    }

    /// The route metrics (IPv4, IPv6) last reapplied on `device`.
    pub fn applied_metrics(&self, device: &str) -> Option<[i64; 2]> {
        lock(&self.state).metrics.get(device).copied()
    }

    pub fn property(&self, path: &str, interface: &str, name: &str) -> Value {
        lock(&self.state).objects[path][interface][name].1.clone()
    }

    pub fn profile_ids(&self) -> Vec<String> {
        lock(&self.state)
            .profiles
            .values()
            .map(|settings| settings["connection"]["id"]["data"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Sets properties of `path` and says so.
    pub fn change(&self, path: &str, interface: &str, changes: &[(&str, &str, Value)]) {
        let changes: Vec<(String, Property)> = changes
            .iter()
            .map(|(name, signature, value)| (name.to_string(), (signature.to_string(), value.clone())))
            .collect();
        set(&self.state, &self.connection, path, interface, changes);
    }

    /// Plays a recorded signal (`recorded/nm-*.jsonl`): what it changes is
    /// changed, and novad hears a `PropertiesChanged` for it.
    pub fn replay(&self, signal: &Value) {
        let data = &signal["payload"]["data"];
        match signal["member"].as_str().unwrap() {
            "PropertiesChanged" => {
                let changes = data[1]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(name, value)| {
                        (name.clone(), (value["type"].as_str().unwrap().to_owned(), value["data"].clone()))
                    })
                    .collect();
                set(
                    &self.state,
                    &self.connection,
                    signal["path"].as_str().unwrap(),
                    data[0].as_str().unwrap(),
                    changes,
                );
            }
            "InterfacesAdded" => {
                let path = data[0].as_str().unwrap();
                for (interface, properties) in data[1].as_object().unwrap() {
                    let properties = properties
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(name, value)| {
                            (name.clone(), (value["type"].as_str().unwrap().to_owned(), value["data"].clone()))
                        })
                        .collect();
                    lock(&self.state).objects.entry(path.into()).or_default().insert(interface.clone(), properties);
                }
            }
            // Its effect comes with the PropertiesChanged that follows.
            _ => {}
        }
    }

    async fn answer(self) {
        let mut stream = MessageStream::from(&self.connection);
        loop {
            let next = poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
            let Some(Ok(message)) = next else { return };
            if message.message_type() != Type::MethodCall {
                continue;
            }
            let header = message.header();
            let path = header.path().map(|path| path.to_string()).unwrap_or_default();
            let interface = header.interface().map(|interface| interface.to_string()).unwrap_or_default();
            let method = header.member().map(|member| member.to_string()).unwrap_or_default();
            lock(&self.state).calls.push((path.clone(), interface.clone(), method.clone()));
            let fails = {
                let mut state = lock(&self.state);
                let fails = state.fail_next.as_deref() == Some(method.as_str());
                if fails {
                    state.fail_next = None;
                }
                fails
            };
            let reply = if fails {
                Err("the connection changed meanwhile".to_owned())
            } else {
                self.call(&message, &path, &interface, &method)
            };
            let sent = match reply {
                Ok(reply) => self.connection.send(&reply.build(&header)).await,
                Err(error) => {
                    self.connection.reply_error(&header, "org.freedesktop.NetworkManager.Failed", &(error,)).await
                }
            };
            sent.unwrap();
        }
    }

    fn call(&self, message: &Message, path: &str, interface: &str, method: &str) -> Result<Reply, String> {
        let body = message.body();
        match (interface, method) {
            (PROPERTIES, "GetAll") => {
                let (wanted,): (String,) = body.deserialize().map_err(|e| e.to_string())?;
                let state = lock(&self.state);
                let properties =
                    state.objects.get(path).and_then(|object| object.get(&wanted)).ok_or("no such object")?;
                Ok(Reply::Properties(
                    properties.iter().map(|(name, property)| (name.clone(), variant(property))).collect(),
                ))
            }
            (PROPERTIES, "Set") => {
                let (target, name, value): (String, String, OwnedVariant) =
                    body.deserialize().map_err(|e| e.to_string())?;
                let enabled = bool::try_from(&value).map_err(|e| e.to_string())?;
                assert_eq!((target.as_str(), name.as_str()), (NM, "WirelessEnabled"), "the only property novad sets");
                set(&self.state, &self.connection, NM_PATH, NM, vec![(name, ("b".into(), json!(enabled)))]);
                Ok(Reply::Empty)
            }
            (NM, "GetDevices") => Ok(Reply::Paths(paths(&self.property(NM_PATH, NM, "Devices")))),
            (SETTINGS, "ListConnections") => {
                Ok(Reply::Paths(lock(&self.state).profiles.keys().map(|path| object_path(path)).collect()))
            }
            (SETTINGS_CONNECTION, "GetSettings") => {
                let state = lock(&self.state);
                let settings = state.profiles.get(path).ok_or("no such profile")?;
                Ok(Reply::Settings(settings_variant(settings)))
            }
            (SETTINGS_CONNECTION, "Update2") => {
                let (settings, _flags, _args): (Settings, u32, HashMap<String, OwnedVariant>) =
                    body.deserialize().map_err(|e| e.to_string())?;
                let psk = settings
                    .get("802-11-wireless-security")
                    .and_then(|security| security.get("psk"))
                    .and_then(|psk| String::try_from(psk.try_clone().ok()?).ok())
                    .ok_or("no password")?;
                lock(&self.state).saved_passwords.insert(path.into(), psk);
                Ok(Reply::Dict)
            }
            (SETTINGS_CONNECTION, "Delete") => {
                let removed = lock(&self.state).profiles.remove(path);
                removed.ok_or("no such profile")?;
                self.signal(SETTINGS_PATH_STR, SETTINGS, "ConnectionRemoved", &(object_path(path),));
                Ok(Reply::Empty)
            }
            (NM, "ActivateConnection") => {
                let (profile, device, _point): (OwnedObjectPath, OwnedObjectPath, OwnedObjectPath) =
                    body.deserialize().map_err(|e| e.to_string())?;
                Ok(Reply::Path(self.activate(profile.as_str(), device.as_str())))
            }
            (NM, "AddAndActivateConnection") => {
                let (settings, device, _point): (Settings, OwnedObjectPath, OwnedObjectPath) =
                    body.deserialize().map_err(|e| e.to_string())?;
                let profile = self.add_profile(&settings);
                let active = self.activate(&profile, device.as_str());
                Ok(Reply::PathPair(object_path(&profile), active))
            }
            (NM, "DeactivateConnection") => {
                let (active,): (OwnedObjectPath,) = body.deserialize().map_err(|e| e.to_string())?;
                self.deactivate(active.as_str(), REASON_USER);
                Ok(Reply::Empty)
            }
            (WIRELESS, "RequestScan") => {
                let last = self.property(path, WIRELESS, "LastScan").as_i64().unwrap();
                set(
                    &self.state,
                    &self.connection,
                    path,
                    WIRELESS,
                    vec![("LastScan".into(), ("x".into(), json!(last + 1000)))],
                );
                Ok(Reply::Empty)
            }
            (DEVICE, "GetAppliedConnection") => {
                let active = self.property(path, DEVICE, "ActiveConnection");
                let profile = self.property(active.as_str().unwrap(), ACTIVE, "Connection");
                let mut settings = lock(&self.state).profiles[profile.as_str().unwrap()].clone();
                if let Some(metrics) = lock(&self.state).metrics.get(path) {
                    for (family, metric) in ["ipv4", "ipv6"].into_iter().zip(metrics) {
                        settings[family]["route-metric"] = json!({"type": "x", "data": metric});
                    }
                }
                Ok(Reply::Applied(settings_variant(&settings), 1))
            }
            (DEVICE, "Reapply") => {
                let (settings, _version, _flags): (Settings, u64, u32) =
                    body.deserialize().map_err(|e| e.to_string())?;
                let metrics = ["ipv4", "ipv6"].map(|family| {
                    settings[family]["route-metric"]
                        .try_clone()
                        .ok()
                        .and_then(|value| i64::try_from(value).ok())
                        .unwrap()
                });
                lock(&self.state).metrics.insert(path.into(), metrics);
                self.choose_primary();
                Ok(Reply::Empty)
            }
            _ => Err(format!("the fake doesn't do {interface}.{method}")),
        }
    }

    fn add_profile(&self, settings: &Settings) -> String {
        let mut state = lock(&self.state);
        state.next_id += 1;
        let path = format!("{SETTINGS_PREFIX}{}", state.next_id);
        let ssid = settings["802-11-wireless"]["ssid"]
            .try_clone()
            .ok()
            .and_then(|ssid| Vec::<u8>::try_from(ssid).ok())
            .unwrap();
        let name = String::from_utf8_lossy(&ssid).into_owned();
        let mut profile = json!({
            "connection": {"id": {"type": "s", "data": name}, "type": {"type": "s", "data": "802-11-wireless"}},
            "802-11-wireless": {"ssid": {"type": "ay", "data": ssid}},
            "ipv4": {"method": {"type": "s", "data": "auto"}},
            "ipv6": {"method": {"type": "s", "data": "auto"}},
        });
        if let Some(security) = settings.get("802-11-wireless-security") {
            let key_mgmt = String::try_from(security["key-mgmt"].try_clone().unwrap()).unwrap();
            profile["802-11-wireless-security"] = json!({"key-mgmt": {"type": "s", "data": key_mgmt}});
            let psk = String::try_from(security["psk"].try_clone().unwrap()).unwrap();
            state.saved_passwords.insert(path.clone(), psk);
        }
        state.profiles.insert(path.clone(), profile);
        path
    }

    /// Activates `profile` on `device`: it connects a moment later, or
    /// fails for missing secrets when its password isn't the network's.
    fn activate(&self, profile: &str, device: &str) -> OwnedObjectPath {
        let (active, works, ssid, stall) = {
            let mut state = lock(&self.state);
            state.next_id += 1;
            let active = format!("{ACTIVE_PREFIX}{}", state.next_id);
            let settings = state.profiles[profile].clone();
            let ssid = settings["802-11-wireless"]["ssid"]["data"]
                .as_array()
                .map(|bytes| bytes.iter().map(|byte| byte.as_u64().unwrap() as u8).collect::<Vec<u8>>())
                .unwrap_or_default();
            let name = String::from_utf8_lossy(&ssid).into_owned();
            let works = match state.passwords.get(&name) {
                Some(password) => state.saved_passwords.get(profile) == Some(password),
                None => true,
            };
            let id = settings["connection"]["id"]["data"].as_str().unwrap().to_owned();
            let kind = settings["connection"]["type"]["data"].as_str().unwrap().to_owned();
            let properties = BTreeMap::from([
                ("Id".to_string(), ("s".to_string(), json!(id))),
                ("Type".to_string(), ("s".to_string(), json!(kind))),
                ("Connection".to_string(), ("o".to_string(), json!(profile))),
                ("Devices".to_string(), ("ao".to_string(), json!([device]))),
                ("State".to_string(), ("u".to_string(), json!(ACTIVATING))),
                ("Default".to_string(), ("b".to_string(), json!(false))),
            ]);
            state.objects.insert(active.clone(), BTreeMap::from([(ACTIVE.to_string(), properties)]));
            (active, works, ssid, state.stall)
        };
        if stall {
            return object_path(&active);
        }
        let connection = self.connection.clone();
        let state = Arc::clone(&self.state);
        let finishing = active.clone();
        let device = device.to_owned();
        // After the reply, as NetworkManager's own state changes come.
        std::thread::spawn(move || {
            nova_test_support::settle(std::time::Duration::from_millis(50));
            let fake = FakeNm { connection, state };
            if works {
                fake.connected(&finishing, &device, &ssid);
            } else {
                fake.state_changed(&finishing, DEACTIVATED, REASON_NO_SECRETS);
                lock(&fake.state).objects.remove(&finishing);
            }
        });
        object_path(&active)
    }

    fn connected(&self, active: &str, device: &str, ssid: &[u8]) {
        let previous = self.property(device, DEVICE, "ActiveConnection");
        if let Some(previous) = previous.as_str().filter(|previous| *previous != NO_OBJECT && *previous != active) {
            self.deactivate(previous, REASON_NONE);
        }
        set(&self.state, &self.connection, active, ACTIVE, vec![("State".into(), ("u".into(), json!(ACTIVATED)))]);
        self.state_changed(active, ACTIVATED, REASON_NONE);
        let point = {
            let state = lock(&self.state);
            state
                .objects
                .iter()
                .find(|(_, interfaces)| {
                    interfaces.get(ACCESS_POINT).is_some_and(|point| point["Ssid"].1 == json!(ssid))
                })
                .map(|(path, _)| path.clone())
        };
        if let Some(point) = point {
            set(
                &self.state,
                &self.connection,
                device,
                WIRELESS,
                vec![("ActiveAccessPoint".into(), ("o".into(), json!(point)))],
            );
        }
        set(
            &self.state,
            &self.connection,
            device,
            DEVICE,
            vec![
                ("ActiveConnection".into(), ("o".into(), json!(active))),
                ("State".into(), ("u".into(), json!(DEVICE_ACTIVATED))),
            ],
        );
        let mut actives: Vec<Value> = self.property(NM_PATH, NM, "ActiveConnections").as_array().unwrap().clone();
        actives.push(json!(active));
        set(
            &self.state,
            &self.connection,
            NM_PATH,
            NM,
            vec![("ActiveConnections".into(), ("ao".into(), json!(actives)))],
        );
        self.choose_primary();
    }

    fn deactivate(&self, active: &str, reason: u32) {
        let devices = self.property(active, ACTIVE, "Devices");
        self.state_changed(active, DEACTIVATED, reason);
        lock(&self.state).objects.remove(active);
        for device in devices.as_array().unwrap() {
            let device = device.as_str().unwrap();
            let mut changes = vec![
                ("ActiveConnection".into(), ("o".into(), json!(NO_OBJECT))),
                ("State".into(), ("u".into(), json!(DEVICE_DISCONNECTED))),
            ];
            if self.has(device, WIRELESS) {
                set(
                    &self.state,
                    &self.connection,
                    device,
                    WIRELESS,
                    vec![("ActiveAccessPoint".into(), ("o".into(), json!(NO_OBJECT)))],
                );
            }
            set(&self.state, &self.connection, device, DEVICE, std::mem::take(&mut changes));
        }
        let actives: Vec<Value> = self
            .property(NM_PATH, NM, "ActiveConnections")
            .as_array()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() != Some(active))
            .cloned()
            .collect();
        set(
            &self.state,
            &self.connection,
            NM_PATH,
            NM,
            vec![("ActiveConnections".into(), ("ao".into(), json!(actives)))],
        );
        self.choose_primary();
    }

    fn has(&self, path: &str, interface: &str) -> bool {
        lock(&self.state).objects.get(path).is_some_and(|object| object.contains_key(interface))
    }

    /// The primary connection: the activated wired or Wi-Fi connection
    /// whose device has the lowest route metric.
    fn choose_primary(&self) {
        let primary = {
            let state = lock(&self.state);
            let actives = state.objects[NM_PATH][NM]["ActiveConnections"].1.as_array().unwrap().clone();
            actives
                .iter()
                .filter_map(|active| {
                    let active = active.as_str()?;
                    let properties = state.objects.get(active)?.get(ACTIVE)?;
                    let kind = properties["Type"].1.as_str()?;
                    if properties["State"].1 != json!(ACTIVATED)
                        || !matches!(kind, "802-3-ethernet" | "802-11-wireless")
                    {
                        return None;
                    }
                    let device = properties["Devices"].1[0].as_str()?.to_owned();
                    let profile = properties["Connection"].1.as_str()?;
                    let default = if kind == "802-3-ethernet" { 100 } else { 600 };
                    let saved = state
                        .profiles
                        .get(profile)
                        .and_then(|settings| settings["ipv4"]["route-metric"]["data"].as_i64());
                    let metric = state
                        .metrics
                        .get(&device)
                        .map(|metrics| metrics[0])
                        .or(saved)
                        .filter(|metric| *metric >= 0)
                        .unwrap_or(default);
                    Some((metric, active.to_owned(), kind.to_owned()))
                })
                .min()
        };
        let (path, kind) = primary.map_or((NO_OBJECT.to_owned(), String::new()), |(_, path, kind)| (path, kind));
        set(
            &self.state,
            &self.connection,
            NM_PATH,
            NM,
            vec![
                ("PrimaryConnection".into(), ("o".into(), json!(path))),
                ("PrimaryConnectionType".into(), ("s".into(), json!(kind))),
            ],
        );
    }

    /// Sends `active`'s `StateChanged`.
    pub fn state_changed(&self, active: &str, state: u32, reason: u32) {
        self.signal(active, ACTIVE, "StateChanged", &(state, reason));
    }

    fn signal<B: serde::Serialize + zbus::wire::DynamicType>(
        &self,
        path: &str,
        interface: &str,
        member: &str,
        body: &B,
    ) {
        zbus::block_on(self.connection.emit_signal(None::<&str>, path, interface, member, body)).unwrap();
    }
}

const SETTINGS_PATH_STR: &str = super::manager::SETTINGS_PATH;

/// Changes properties of `path` and sends `PropertiesChanged` for them.
fn set(state: &Mutex<Fake>, connection: &Connection, path: &str, interface: &str, changes: Vec<(String, Property)>) {
    let changed: HashMap<String, OwnedVariant> = {
        let mut state = lock(state);
        let properties = state.objects.entry(path.into()).or_default().entry(interface.into()).or_default();
        changes
            .into_iter()
            .map(|(name, property)| {
                let value = variant(&property);
                properties.insert(name.clone(), property);
                (name, value)
            })
            .collect()
    };
    let body = (interface, changed, Vec::<String>::new());
    zbus::block_on(connection.emit_signal(None::<&str>, path, PROPERTIES, "PropertiesChanged", &body)).unwrap();
}

enum Reply {
    Empty,
    Dict,
    Path(OwnedObjectPath),
    PathPair(OwnedObjectPath, OwnedObjectPath),
    Paths(Vec<OwnedObjectPath>),
    Properties(HashMap<String, OwnedVariant>),
    Settings(Settings),
    Applied(Settings, u64),
}

impl Reply {
    fn build(self, header: &zbus::message::Header<'_>) -> Message {
        let reply = Message::method_return(header);
        match self {
            Reply::Empty => reply.build(&()),
            Reply::Dict => reply.build(&(HashMap::<String, OwnedVariant>::new(),)),
            Reply::Path(path) => reply.build(&(path,)),
            Reply::PathPair(first, second) => reply.build(&(first, second)),
            Reply::Paths(paths) => reply.build(&(paths,)),
            Reply::Properties(properties) => reply.build(&(properties,)),
            Reply::Settings(settings) => reply.build(&(settings,)),
            Reply::Applied(settings, version) => reply.build(&(settings, version)),
        }
        .unwrap()
    }
}

fn object_path(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path).unwrap()
}

fn paths(value: &Value) -> Vec<OwnedObjectPath> {
    value.as_array().unwrap().iter().map(|path| object_path(path.as_str().unwrap())).collect()
}

/// A recorded `GetSettings` as D-Bus values.
fn settings_variant(settings: &Value) -> Settings {
    settings
        .as_object()
        .unwrap()
        .iter()
        .map(|(setting, keys)| {
            let keys = keys
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| {
                    (key.clone(), variant(&(value["type"].as_str().unwrap().to_owned(), value["data"].clone())))
                })
                .collect();
            (setting.clone(), keys)
        })
        .collect()
}

/// A recorded value with its signature, as a D-Bus value. Only the
/// signatures NetworkManager's recordings hold.
fn variant((signature, value): &Property) -> OwnedVariant {
    let owned = |variant: Variant<'_>| OwnedVariant::try_from(variant).unwrap();
    let text = || value.as_str().unwrap().to_owned();
    let unsigned = || value.as_u64().unwrap();
    match signature.as_str() {
        "s" => owned(Variant::from(text())),
        "o" => owned(Variant::from(object_path(&text()))),
        "b" => owned(Variant::from(value.as_bool().unwrap())),
        "y" => owned(Variant::from(unsigned() as u8)),
        "u" => owned(Variant::from(unsigned() as u32)),
        "t" => owned(Variant::from(unsigned())),
        "i" => owned(Variant::from(value.as_i64().unwrap() as i32)),
        "x" => owned(Variant::from(value.as_i64().unwrap())),
        "ay" => owned(Variant::from(
            value.as_array().unwrap().iter().map(|byte| byte.as_u64().unwrap() as u8).collect::<Vec<u8>>(),
        )),
        "au" => owned(Variant::from(
            value.as_array().unwrap().iter().map(|n| n.as_u64().unwrap() as u32).collect::<Vec<u32>>(),
        )),
        "as" => owned(Variant::from(
            value.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect::<Vec<String>>(),
        )),
        "ao" => owned(Variant::from(paths(value))),
        "(uu)" => {
            let pair = value.as_array().unwrap();
            owned(Variant::from(Structure::from((pair[0].as_u64().unwrap() as u32, pair[1].as_u64().unwrap() as u32))))
        }
        "a{sv}" => {
            let dict: HashMap<String, OwnedVariant> = value
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| {
                    (key.clone(), variant(&(value["type"].as_str().unwrap().to_owned(), value["data"].clone())))
                })
                .collect();
            OwnedVariant::from(dict)
        }
        "a{ss}" => {
            let dict: HashMap<String, String> = value
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_owned()))
                .collect();
            OwnedVariant::from(dict)
        }
        "aa{sv}" => {
            assert!(value.as_array().unwrap().is_empty(), "only empty lists of dicts are recorded");
            owned(Variant::from(Vec::<HashMap<String, OwnedVariant>>::new()))
        }
        other => panic!("the fake can't serve a value of type {other}"),
    }
}
