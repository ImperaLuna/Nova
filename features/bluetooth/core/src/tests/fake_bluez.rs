//! The stand-in for BlueZ: an adapter and its devices on the test's bus,
//! as BlueZ's D-Bus API describes them.

use super::*;

/// What the fake's `Pair` asks the agent.
#[derive(Clone, Copy)]
pub(super) enum Pairing {
    /// `RequestConfirmation` with this passkey.
    Confirm(u32),
    /// `DisplayPasskey`, then pairs once the test says the code was typed.
    Show(u32),
}

/// The fake's state, shared by its objects.
pub(super) struct Fake {
    pub(super) objects: BTreeMap<String, Interfaces>,
    /// Every call novad made, like `Pair /org/bluez/hci0/dev_…`.
    pub(super) calls: Vec<String>,
    /// The agent novad registered: its bus name and path.
    pub(super) agent: Option<(String, String)>,
    pub(super) pairing: Pairing,
    /// `/sys/class/rfkill/rfkill0/soft`: `Powered` is refused while it says 1.
    pub(super) soft_block: PathBuf,
    /// How many `StartDiscovery` calls to refuse, as an adapter still
    /// coming up does.
    pub(super) refuse_discovery: usize,
    /// Whether `Connect` fails as BlueZ does when it tried LE and gave up.
    pub(super) abort_le: bool,
    /// How many times a device that plays music connects without it, as
    /// a headset that refuses A2DP right after connecting does.
    pub(super) refuse_a2dp: usize,
    /// A step that waits until a `Disconnect` or `CancelPairing` releases
    /// it: a `Connect` to a device that doesn't answer (BlueZ cancels it
    /// on `Disconnect`; powering off is refused meanwhile), or a slow
    /// `Trusted` write.
    pub(super) held: Option<Held>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Hold {
    Connect,
    Trust,
}

pub(super) struct Held {
    pub(super) step: Hold,
    pub(super) waiting: bool,
    pub(super) cancelled: bool,
    pub(super) waker: Option<std::task::Waker>,
}

pub(super) type Shared = Arc<Mutex<Fake>>;

pub(super) struct FakeBlueZ {
    pub(super) connection: Connection,
    pub(super) fake: Shared,
}

impl FakeBlueZ {
    pub(super) fn start(connection: Connection, soft_block: PathBuf) -> Self {
        let objects = objects();
        let fake = Arc::new(Mutex::new(Fake {
            objects: objects.clone(),
            calls: Vec::new(),
            agent: None,
            pairing: Pairing::Confirm(123_456),
            refuse_discovery: 0,
            abort_le: false,
            refuse_a2dp: 0,
            held: None,
            soft_block,
        }));
        zbus::block_on(async {
            let server = connection.object_server();
            server.at("/", Manager(Arc::clone(&fake))).await.unwrap();
            server.at(BLUEZ_PATH, AgentManager(Arc::clone(&fake))).await.unwrap();
            server.at(ADAPTER_PATH, FakeAdapter(Arc::clone(&fake))).await.unwrap();
            for path in objects.keys().filter(|path| path.contains("/dev_")) {
                server.at(path.as_str(), FakeDevice { fake: Arc::clone(&fake), path: path.clone() }).await.unwrap();
            }
        });
        zbus::block_on(connection.request_name(BLUEZ)).unwrap();
        Self { connection, fake }
    }

    pub(super) fn calls(&self) -> Vec<String> {
        lock(&self.fake).calls.clone()
    }

    /// Changes a property, and says so as BlueZ does.
    pub(super) fn change(&self, path: &str, interface: &str, name: &str, value: OwnedVariant) {
        zbus::block_on(changed(&self.connection, &self.fake, (path, interface, name), value));
    }

    /// A property goes, as BlueZ says it (`RSSI` when discovery stops).
    pub(super) fn invalidate(&self, path: &str, interface: &str, name: &str) {
        lock(&self.fake).objects.get_mut(path).unwrap().get_mut(interface).unwrap().remove(name);
        let changed: HashMap<String, OwnedVariant> = HashMap::new();
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            path,
            PROPERTIES,
            "PropertiesChanged",
            &(interface, changed, vec![name.to_owned()]),
        ))
        .unwrap();
    }

    /// A device goes, as BlueZ says it.
    pub(super) fn remove(&self, path: &str) {
        let interfaces: Vec<String> = lock(&self.fake).objects.remove(path).unwrap().into_keys().collect();
        let path = ObjectPath::try_from(path).unwrap();
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            "/",
            OBJECT_MANAGER,
            "InterfacesRemoved",
            &(path, interfaces),
        ))
        .unwrap();
    }

    /// The device at `address` starts pairing by itself (a phone pairing
    /// with this computer): BlueZ asks the agent to confirm `passkey`.
    pub(super) fn confirm_from(&self, address: &str, passkey: u32) -> zbus::Result<()> {
        let device = ObjectPath::try_from(device_path(address)).unwrap();
        zbus::block_on(ask_agent(&self.connection, &self.fake, "RequestConfirmation", &(device, passkey)))
    }
}

/// Sets the property `(path, interface, name)` to `value` and says so on the
/// bus.
pub(super) async fn changed(connection: &Connection, fake: &Shared, property: (&str, &str, &str), value: OwnedVariant) {
    let (path, interface, name) = property;
    let properties = {
        let mut fake = lock(fake);
        let properties = fake.objects.get_mut(path).unwrap().get_mut(interface).unwrap();
        properties.insert(name.into(), value.try_clone().unwrap());
        HashMap::from([(name.to_owned(), value)])
    };
    connection
        .emit_signal(
            None::<&str>,
            path,
            PROPERTIES,
            "PropertiesChanged",
            &(interface, properties, Vec::<String>::new()),
        )
        .await
        .unwrap();
}

pub(super) async fn ask_agent<B: serde::Serialize + zbus::wire::DynamicType>(
    connection: &Connection,
    fake: &Shared,
    method: &str,
    body: &B,
) -> zbus::Result<()> {
    let (name, path) = lock(fake).agent.clone().expect("an agent registered");
    connection.call_method(Some(name.as_str()), path.as_str(), Some("org.bluez.Agent1"), method, body).await.map(|_| ())
}

/// The errors the fake answers with, named as BlueZ names them.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub(super) enum BlueZError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Blocked(String),
    NotReady(String),
    AuthenticationRejected(String),
    AuthenticationCanceled(String),
    Failed(String),
    Busy(String),
}

pub(super) struct Manager(Shared);

#[interface(name = "org.freedesktop.DBus.ObjectManager")]
impl Manager {
    fn get_managed_objects(&self) -> HashMap<OwnedObjectPath, Interfaces> {
        let fake = lock(&self.0);
        fake.objects
            .iter()
            .map(|(path, interfaces)| {
                let interfaces = interfaces
                    .iter()
                    .map(|(name, properties)| {
                        let properties =
                            properties.iter().map(|(key, value)| (key.clone(), value.try_clone().unwrap())).collect();
                        (name.clone(), properties)
                    })
                    .collect();
                (OwnedObjectPath::try_from(path.as_str()).unwrap(), interfaces)
            })
            .collect()
    }
}

pub(super) struct AgentManager(Shared);

#[interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(
        &self,
        agent: OwnedObjectPath,
        capability: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        let mut fake = lock(&self.0);
        fake.calls.push(format!("RegisterAgent {agent} {capability}"));
        fake.agent = Some((header.sender().unwrap().to_string(), agent.to_string()));
    }

    fn request_default_agent(&self, agent: OwnedObjectPath) {
        lock(&self.0).calls.push(format!("RequestDefaultAgent {agent}"));
    }

    fn unregister_agent(&self, agent: OwnedObjectPath) {
        lock(&self.0).calls.push(format!("UnregisterAgent {agent}"));
    }
}

pub(super) struct FakeAdapter(Shared);

#[interface(name = "org.bluez.Adapter1")]
impl FakeAdapter {
    async fn start_discovery(&self, #[zbus(connection)] connection: &Connection) -> Result<(), BlueZError> {
        {
            let mut fake = lock(&self.0);
            fake.calls.push("StartDiscovery".into());
            if fake.refuse_discovery > 0 {
                fake.refuse_discovery -= 1;
                return Err(BlueZError::NotReady("Resource Not Ready".into()));
            }
        }
        changed(connection, &self.0, (ADAPTER_PATH, ADAPTER, "Discovering"), OwnedVariant::from(true)).await;
        Ok(())
    }

    async fn stop_discovery(&self, #[zbus(connection)] connection: &Connection) {
        lock(&self.0).calls.push("StopDiscovery".into());
        changed(connection, &self.0, (ADAPTER_PATH, ADAPTER, "Discovering"), OwnedVariant::from(false)).await;
    }

    async fn remove_device(&self, device: OwnedObjectPath, #[zbus(connection)] connection: &Connection) {
        let interfaces: Vec<String> = {
            let mut fake = lock(&self.0);
            fake.calls.push(format!("RemoveDevice {device}"));
            fake.objects.remove(device.as_str()).unwrap().into_keys().collect()
        };
        connection
            .emit_signal(None::<&str>, "/", OBJECT_MANAGER, "InterfacesRemoved", &(device, interfaces))
            .await
            .unwrap();
    }

    #[zbus(property)]
    fn powered(&self) -> bool {
        bool::try_from(&lock(&self.0).objects[ADAPTER_PATH][ADAPTER]["Powered"]).unwrap()
    }

    /// Refused while blocked, like BlueZ.
    #[zbus(property)]
    async fn set_powered(&self, on: bool, #[zbus(connection)] connection: &Connection) -> Result<(), BlueZError> {
        let blocked = {
            let mut fake = lock(&self.0);
            fake.calls.push(format!("Powered {on}"));
            if fake.held.as_ref().is_some_and(|held| held.waiting && !held.cancelled) {
                return Err(BlueZError::Busy("Operation already in progress".into()));
            }
            std::fs::read_to_string(&fake.soft_block).is_ok_and(|soft| soft.trim() == "1")
        };
        if on && blocked {
            return Err(BlueZError::Blocked("Blocked through rfkill".into()));
        }
        changed(connection, &self.0, (ADAPTER_PATH, ADAPTER, "Powered"), OwnedVariant::from(on)).await;
        Ok(())
    }
}

pub(super) struct FakeDevice {
    fake: Shared,
    path: String,
}

impl FakeDevice {
    fn log(&self, call: &str) {
        lock(&self.fake).calls.push(format!("{call} {}", self.path));
    }

    /// Waits, when the test holds this step, until it's released; true
    /// when it was.
    async fn hold(&self, step: Hold) -> bool {
        match lock(&self.fake).held.as_mut() {
            Some(held) if held.step == step => held.waiting = true,
            _ => return false,
        }
        poll_fn(|cx| {
            let mut fake = lock(&self.fake);
            let held = fake.held.as_mut().unwrap();
            if held.cancelled {
                return Poll::Ready(true);
            }
            held.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }

    /// Where BlueZ puts the A2DP transport of a device that plays music.
    fn transport(&self) -> String {
        format!("{}/sep1/fd0", self.path)
    }

    /// A device that plays music brings its A2DP transport up, unless the
    /// test has it refuse: BlueZ adds the transport under the device.
    async fn music_up(&self, connection: &Connection) {
        let interfaces = {
            let mut fake = lock(&self.fake);
            let uuids = fake.objects[&self.path][DEVICE]
                .get("UUIDs")
                .map(|uuids| Vec::<String>::try_from(uuids.try_clone().unwrap()).unwrap());
            if !uuids.is_some_and(|uuids| uuids.iter().any(|uuid| uuid == A2DP_SINK))
                || fake.objects.contains_key(&self.transport())
            {
                return;
            }
            if fake.refuse_a2dp > 0 {
                fake.refuse_a2dp -= 1;
                return;
            }
            let transport: Properties = HashMap::from([
                // This computer's end: the source, playing to the device.
                ("UUID".into(), text(bluez::A2DP_SOURCE)),
                ("Device".into(), path(&self.path)),
                ("State".into(), text("idle")),
            ]);
            let interfaces: Interfaces = HashMap::from([(MEDIA_TRANSPORT.to_owned(), transport)]);
            fake.objects.insert(self.transport(), interfaces.clone());
            interfaces
        };
        let transport = ObjectPath::try_from(self.transport()).unwrap();
        connection
            .emit_signal(None::<&str>, "/", OBJECT_MANAGER, "InterfacesAdded", &(transport, interfaces))
            .await
            .unwrap();
    }

    /// Disconnected, its transport goes.
    async fn music_down(&self, connection: &Connection) {
        if lock(&self.fake).objects.remove(&self.transport()).is_none() {
            return;
        }
        let transport = ObjectPath::try_from(self.transport()).unwrap();
        let interfaces = vec![MEDIA_TRANSPORT.to_owned()];
        connection
            .emit_signal(None::<&str>, "/", OBJECT_MANAGER, "InterfacesRemoved", &(transport, interfaces))
            .await
            .unwrap();
    }

    /// Releases a held step; true when one was waiting.
    fn release(&self) -> bool {
        let mut fake = lock(&self.fake);
        let Some(held) = fake.held.as_mut().filter(|held| held.waiting) else { return false };
        held.cancelled = true;
        if let Some(waker) = held.waker.take() {
            waker.wake();
        }
        true
    }
}

#[interface(name = "org.bluez.Device1")]
impl FakeDevice {
    /// Asks the agent as BlueZ would, then pairs, or fails as BlueZ does.
    async fn pair(&self, #[zbus(connection)] connection: &Connection) -> Result<(), BlueZError> {
        self.log("Pair");
        let device = ObjectPath::try_from(self.path.as_str()).unwrap();
        let pairing = lock(&self.fake).pairing;
        let asked = match pairing {
            Pairing::Confirm(passkey) => {
                ask_agent(connection, &self.fake, "RequestConfirmation", &(device, passkey)).await
            }
            Pairing::Show(passkey) => {
                ask_agent(connection, &self.fake, "DisplayPasskey", &(device, passkey, 0u16)).await.unwrap();
                // Until the test types it: it sets the property itself.
                wait_until(WAIT, "the code typed on the device", || {
                    lock(&self.fake).calls.iter().any(|call| call == "typed")
                });
                Ok(())
            }
        };
        match asked {
            Ok(()) => {
                changed(connection, &self.fake, (&self.path, DEVICE, "Paired"), OwnedVariant::from(true)).await;
                Ok(())
            }
            Err(zbus::Error::MethodError(name, ..)) if name.as_str() == "org.bluez.Error.Rejected" => {
                Err(BlueZError::AuthenticationRejected("Authentication Rejected".into()))
            }
            Err(_) => Err(BlueZError::AuthenticationCanceled("Authentication Canceled".into())),
        }
    }

    async fn connect(&self, #[zbus(connection)] connection: &Connection) -> Result<(), BlueZError> {
        self.log("Connect");
        if lock(&self.fake).abort_le {
            return Err(BlueZError::Failed("le-connection-abort-by-local".into()));
        }
        if self.hold(Hold::Connect).await {
            return Err(BlueZError::Failed("br-connection-canceled".into()));
        }
        changed(connection, &self.fake, (&self.path, DEVICE, "Connected"), OwnedVariant::from(true)).await;
        self.music_up(connection).await;
        Ok(())
    }

    async fn connect_profile(&self, uuid: String, #[zbus(connection)] connection: &Connection) {
        self.log(&format!("ConnectProfile {uuid}"));
        changed(connection, &self.fake, (&self.path, DEVICE, "Connected"), OwnedVariant::from(true)).await;
        if uuid == A2DP_SINK {
            self.music_up(connection).await;
        }
    }

    async fn disconnect(&self, #[zbus(connection)] connection: &Connection) {
        self.log("Disconnect");
        if self.release() {
            return;
        }
        changed(connection, &self.fake, (&self.path, DEVICE, "Connected"), OwnedVariant::from(false)).await;
        self.music_down(connection).await;
    }

    fn cancel_pairing(&self) {
        self.log("CancelPairing");
        // Pairing is over by now: there's nothing for BlueZ to cancel.
        self.release();
    }

    #[zbus(property)]
    fn trusted(&self) -> bool {
        bool::try_from(&lock(&self.fake).objects[&self.path][DEVICE]["Trusted"]).unwrap()
    }

    #[zbus(property)]
    async fn set_trusted(&self, trusted: bool, #[zbus(connection)] connection: &Connection) {
        self.log(&format!("Trusted {trusted}"));
        self.hold(Hold::Trust).await;
        changed(connection, &self.fake, (&self.path, DEVICE, "Trusted"), OwnedVariant::from(trusted)).await;
    }
}
