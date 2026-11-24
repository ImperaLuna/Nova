use super::*;

#[derive(Clone, Copy)]
pub(super) enum Pairing {
    Confirm(u32),
    Show(u32),
}

pub(super) struct Fake {
    pub(super) objects: BTreeMap<String, Interfaces>,
    pub(super) calls: Vec<String>,
    pub(super) agent: Option<(String, String)>,
    pub(super) pairing: Pairing,
    pub(super) soft_block: PathBuf,
    pub(super) refuse_discovery: usize,
    pub(super) abort_le: bool,
    pub(super) refuse_a2dp: usize,
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

    pub(super) fn change(&self, path: &str, interface: &str, name: &str, value: OwnedVariant) {
        zbus::block_on(changed(&self.connection, &self.fake, (path, interface, name), value));
    }

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

    pub(super) fn confirm_from(&self, address: &str, passkey: u32) -> zbus::Result<()> {
        let device = ObjectPath::try_from(device_path(address)).unwrap();
        zbus::block_on(ask_agent(&self.connection, &self.fake, "RequestConfirmation", &(device, passkey)))
    }
}

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

    fn transport(&self) -> String {
        format!("{}/sep1/fd0", self.path)
    }

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
