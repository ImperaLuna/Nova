//! The sidebar's drives against a stand-in for udisks2 only: a private
//! `dbus-daemon` per test with a fake `org.freedesktop.UDisks2` serving
//! the objects in `recorded/udisks.json`, mounting into the test's folder
//! and refusing to unmount while told it's busy, as udisksd does.

use super::*;
use crate::hub::{ClientQueue, Outgoing, next_named};
use crate::icons::Icons;
use crate::notifications;
use crate::session_bus::TestBus;
use nova_protocol::Topic;
use nova_test_support::TempDir;
use serde_json::Value;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::Receiver;
use zbus::interface;
use zbus::object_server::SignalEmitter;

const WAIT: Duration = Duration::from_secs(15);
const STICK_DRIVE: &str = "/org/freedesktop/UDisks2/drives/SanDisk_Cruzer_Blade_4C530001230512115234";

fn recorded() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/files/recorded/udisks.json");
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

type Interfaces = HashMap<String, HashMap<String, OwnedVariant>>;

/// A recorded object's interfaces as the bus carries them; mount folders
/// other than `/` are made under `home`.
fn interfaces(object: &Value, home: &Path) -> Interfaces {
    let variant = |name: &str, value: &Value| -> OwnedVariant {
        let variant = match (name, value) {
            ("Size", Value::Number(size)) => Variant::from(size.as_u64().unwrap()),
            ("Drive", Value::String(path)) => Variant::from(ObjectPath::try_from(path.clone()).unwrap()),
            ("MountPoints", Value::Array(points)) => {
                let points: Vec<Vec<u8>> =
                    points.iter().map(|point| mount_point(point.as_str().unwrap(), home)).collect();
                Variant::from(points)
            }
            (_, Value::Bool(flag)) => Variant::from(*flag),
            (_, Value::String(text)) => Variant::from(text.clone()),
            _ => panic!("{name} isn't a property the fake knows"),
        };
        OwnedVariant::try_from(variant).unwrap()
    };
    object
        .as_object()
        .unwrap()
        .iter()
        .map(|(interface, properties)| {
            let properties =
                properties.as_object().unwrap().iter().map(|(name, value)| (name.clone(), variant(name, value)));
            (interface.clone(), properties.collect())
        })
        .collect()
}

fn copy(interfaces: &Interfaces) -> Interfaces {
    interfaces
        .iter()
        .map(|(interface, properties)| {
            let properties = properties.iter().map(|(name, value)| (name.clone(), value.try_clone().unwrap()));
            (interface.clone(), properties.collect())
        })
        .collect()
}

/// `point` as `MountPoints` holds it: ending in a NUL, under `home`
/// unless it's absolute.
fn mount_point(point: &str, home: &Path) -> Vec<u8> {
    let path = if point.starts_with('/') { PathBuf::from(point) } else { home.join(point) };
    std::fs::create_dir_all(home).unwrap();
    if !point.starts_with('/') {
        std::fs::create_dir_all(&path).unwrap();
    }
    let mut bytes = path.into_os_string().into_vec();
    bytes.push(0);
    bytes
}

/// udisks2 as the fake holds it.
#[derive(Default)]
struct Model {
    objects: HashMap<String, Interfaces>,
    /// Unmounts fail with `DeviceBusy` while set.
    busy: bool,
    /// Where a mount goes.
    media: PathBuf,
    powered_off: Vec<String>,
}

type SharedModel = Arc<Mutex<Model>>;

struct Fixture {
    dir: TempDir,
    bus: TestBus,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    client: ClientId,
    _ui_stream: UnixStream,
    udisks: Connection,
    model: SharedModel,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("udisks-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let client = hub.register(queue);
        hub.subscribe(client, Topic::Files);
        let home = dir.join("home");
        let mut objects = HashMap::new();
        for (path, object) in recorded()["objects"].as_object().unwrap() {
            objects.insert(path.clone(), interfaces(object, &home));
        }
        let model = Arc::new(Mutex::new(Model { objects, media: home.join("media"), ..Model::default() }));
        let udisks = bus.connect();
        zbus::block_on(async {
            let server = udisks.object_server();
            server.at(UDISKS_PATH, Manager(Arc::clone(&model))).await.unwrap();
            let paths: Vec<String> = lock(&model).objects.keys().cloned().collect();
            for path in paths {
                serve(&udisks, &model, &path).await;
            }
        });
        zbus::block_on(udisks.request_name(UDISKS)).unwrap();
        Self { dir, bus, hub, logger, ui, client, _ui_stream: ui_stream, udisks, model }
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn start(&self) -> Drives {
        let bus = Bus::Address(self.bus.address.clone());
        Drives::start(bus, Some(self.home()), Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    /// The names of the drives of the next list sent.
    fn next(&self) -> Vec<Value> {
        next_named(&self.ui, "files_drives", WAIT)["drives"].as_array().unwrap().clone()
    }

    /// The next list sent that `wanted` picks: how full `/` is changes
    /// whenever anything writes to it, which sends the list again.
    fn next_where(&self, wanted: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        loop {
            let drives = self.next();
            if wanted(&drives) {
                return drives;
            }
        }
    }

    /// The stick is plugged in, as udisksd says it.
    fn plug(&self) {
        let home = self.home();
        // The drive first, then its block device, as udisksd adds them.
        let stick = recorded()["stick"].as_object().unwrap().clone();
        let mut objects: Vec<_> = stick.iter().collect();
        objects.sort_by_key(|(path, _)| !path.contains("/drives/"));
        for (path, object) in objects {
            let added = interfaces(object, &home);
            lock(&self.model).objects.insert(path.clone(), added);
            zbus::block_on(serve(&self.udisks, &self.model, path));
            let added = copy(&lock(&self.model).objects[path]);
            let body = (ObjectPath::try_from(path.as_str()).unwrap(), added);
            zbus::block_on(self.udisks.emit_signal(
                None::<&str>,
                UDISKS_PATH,
                OBJECT_MANAGER,
                "InterfacesAdded",
                &body,
            ))
            .unwrap();
        }
    }

    /// A notification server on the test's bus, with Safe To Remove going to it.
    fn notifications(&self, drives: &Drives) -> Arc<Notifications> {
        let sources = notifications::Sources {
            bus: Bus::Address(self.bus.address.clone()),
            state_file: self.dir.join("notifications.json"),
            images: self.dir.join("images"),
            icons: Arc::new(Icons::new(crate::icons::HICOLOR.into(), &[], None)),
        };
        let server = Notifications::start(
            sources,
            crate::apps::AppIndex::of(None, &self.logger),
            Arc::clone(&self.hub),
            Arc::clone(&self.logger),
        );
        self.hub.subscribe(self.client, Topic::Notifications);
        drives.set_notifications(Some(&server));
        server
    }
}

/// Serves the object `path` holds, with the methods of its interfaces.
async fn serve(connection: &Connection, model: &SharedModel, path: &str) {
    let interfaces: Vec<String> = lock(model).objects[path].keys().cloned().collect();
    let server = connection.object_server();
    if interfaces.iter().any(|interface| interface == FILESYSTEM) {
        server.at(path, Filesystem { model: Arc::clone(model), path: path.into() }).await.unwrap();
    }
    if interfaces.iter().any(|interface| interface == DRIVE) {
        server.at(path, DriveObject { model: Arc::clone(model), path: path.into() }).await.unwrap();
    }
}

/// Says that `interface` of `path` changed, as udisksd does: the new values.
async fn changed(emitter: &SignalEmitter<'_>, model: &SharedModel, interface: &str, names: &[&str]) {
    let path = emitter.path().to_string();
    let values: HashMap<String, OwnedVariant> = {
        let model = lock(model);
        names
            .iter()
            .map(|name| (name.to_string(), model.objects[&path][interface][*name].try_clone().unwrap()))
            .collect()
    };
    let body = (interface, values, Vec::<String>::new());
    emitter
        .connection()
        .emit_signal(None::<&str>, path.as_str(), PROPERTIES, "PropertiesChanged", &body)
        .await
        .unwrap();
}

/// The errors the fake answers with, named as udisks2 names them.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.UDisks2.Error")]
enum UDisksError {
    #[zbus(error)]
    ZBus(zbus::Error),
    DeviceBusy(String),
}

struct Manager(SharedModel);

#[interface(name = "org.freedesktop.DBus.ObjectManager")]
impl Manager {
    fn get_managed_objects(&self) -> HashMap<OwnedObjectPath, Interfaces> {
        lock(&self.0)
            .objects
            .iter()
            .map(|(path, interfaces)| {
                let copy = copy(interfaces);
                (OwnedObjectPath::try_from(path.as_str()).unwrap(), copy)
            })
            .collect()
    }
}

struct Filesystem {
    model: SharedModel,
    path: String,
}

#[interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl Filesystem {
    async fn mount(
        &self,
        _options: HashMap<String, OwnedVariant>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> String {
        let folder = {
            let mut model = lock(&self.model);
            let label = <&str>::try_from(&model.objects[&self.path][BLOCK]["IdLabel"]).unwrap().to_string();
            let folder = model.media.join(label);
            std::fs::create_dir_all(&folder).unwrap();
            let mut point = folder.clone().into_os_string().into_vec();
            point.push(0);
            let points = OwnedVariant::try_from(Variant::from(vec![point])).unwrap();
            model
                .objects
                .get_mut(&self.path)
                .unwrap()
                .get_mut(FILESYSTEM)
                .unwrap()
                .insert("MountPoints".into(), points);
            folder
        };
        changed(&emitter, &self.model, FILESYSTEM, &["MountPoints"]).await;
        folder.display().to_string()
    }

    async fn unmount(
        &self,
        _options: HashMap<String, OwnedVariant>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), UDisksError> {
        {
            let mut model = lock(&self.model);
            if model.busy {
                return Err(UDisksError::DeviceBusy("Error unmounting /dev/sdb1: target is busy".into()));
            }
            let none = OwnedVariant::try_from(Variant::from(Vec::<Vec<u8>>::new())).unwrap();
            model.objects.get_mut(&self.path).unwrap().get_mut(FILESYSTEM).unwrap().insert("MountPoints".into(), none);
        }
        changed(&emitter, &self.model, FILESYSTEM, &["MountPoints"]).await;
        Ok(())
    }
}

struct DriveObject {
    model: SharedModel,
    path: String,
}

#[interface(name = "org.freedesktop.UDisks2.Drive")]
impl DriveObject {
    /// The drive and its block devices go, as when it's unplugged.
    async fn power_off(
        &self,
        _options: HashMap<String, OwnedVariant>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let gone: Vec<(String, Vec<String>)> = {
            let mut model = lock(&self.model);
            model.powered_off.push(self.path.clone());
            let drive = self.path.clone();
            let on_it: Vec<String> = model
                .objects
                .iter()
                .filter(|(path, interfaces)| {
                    **path == drive
                        || interfaces
                            .get(BLOCK)
                            .is_some_and(|block| <&ObjectPath>::try_from(&block["Drive"]).unwrap().as_str() == drive)
                })
                .map(|(path, _)| path.clone())
                .collect();
            on_it
                .into_iter()
                .map(|path| {
                    let interfaces = model.objects.remove(&path).unwrap().into_keys().collect();
                    (path, interfaces)
                })
                .collect()
        };
        for (path, interfaces) in gone {
            let body = (ObjectPath::try_from(path.as_str()).unwrap(), interfaces);
            emitter
                .connection()
                .emit_signal(None::<&str>, UDISKS_PATH, OBJECT_MANAGER, "InterfacesRemoved", &body)
                .await
                .unwrap();
        }
    }
}

fn names(drives: &[Value]) -> Vec<&str> {
    drives.iter().map(|drive| drive["name"].as_str().unwrap()).collect()
}

#[test]
fn the_drives_are_listed_and_a_stick_comes_when_plugged() {
    let fixture = Fixture::new("listed");
    let _drives = fixture.start();
    let drives = fixture.next();
    assert_eq!(names(&drives), ["Games SSD", "System"], "no boot partition, no stick yet");
    assert!(drives.iter().all(|drive| drive["used"].is_u64()), "mounted drives say how full they are");
    fixture.plug();
    let drives = fixture.next_where(|drives| drives.len() == 3);
    assert_eq!(names(&drives), ["Games SSD", "System", "SANDISK"]);
    assert_eq!(drives[2]["removable"], true);
    assert!(drives[2].get("mount").is_none());
}

#[test]
fn a_stick_mounts_and_ejects_with_a_safe_to_remove_notification() {
    let fixture = Fixture::new("eject");
    fixture.plug();
    let drives = fixture.start();
    let server = fixture.notifications(&drives);
    assert_eq!(names(&fixture.next()), ["Games SSD", "System", "SANDISK"]);

    let mount = drives.mount("sdb1").unwrap();
    assert_eq!(Path::new(&mount), fixture.home().join("media/SANDISK"));
    let stick = fixture.next_where(|drives| drives[2].get("mount").is_some()).pop().unwrap();
    assert_eq!(stick["mount"].as_str(), Some(mount.as_str()));
    assert!(stick["used"].is_u64());
    assert_eq!(drives.mount("sdb1").unwrap(), mount, "mounting a mounted drive answers where it is");

    assert_eq!(drives.eject("sdb1").unwrap(), EjectOutcome::Ejected);
    assert_eq!(lock(&fixture.model).powered_off, [STICK_DRIVE]);
    let popup = next_named(&fixture.ui, "notification_popup", WAIT);
    assert_eq!(popup["summary"], "Safe To Remove Hardware");
    assert_eq!(popup["body"], "The \u{2018}SANDISK\u{2019} device can now be safely removed from the computer.");
    nova_test_support::wait_until(WAIT, "the stick leaves the list", || {
        lock(&drives.shared.state).listed.iter().all(|drive| drive.id != "sdb1")
    });
    drop(server);
}

#[test]
fn a_busy_stick_stays_mounted() {
    let fixture = Fixture::new("busy");
    fixture.plug();
    let drives = fixture.start();
    fixture.next();
    drives.mount("sdb1").unwrap();
    fixture.next_where(|drives| drives[2].get("mount").is_some());
    lock(&fixture.model).busy = true;
    assert_eq!(drives.eject("sdb1").unwrap(), EjectOutcome::Busy);
    assert!(lock(&fixture.model).powered_off.is_empty());
    assert!(lock(&drives.shared.state).listed.iter().any(|drive| drive.id == "sdb1" && drive.mount.is_some()));
}

#[test]
fn only_listed_removable_drives_eject() {
    let fixture = Fixture::new("refused");
    let drives = fixture.start();
    fixture.next();
    assert!(drives.eject("nvme1n1p1").unwrap_err().contains("can't be ejected"));
    assert!(drives.eject("nvme0n1p1").unwrap_err().contains("no drive"), "the boot partition isn't listed");
    assert!(drives.mount("../sdb1").unwrap_err().contains("no drive"));
}

#[test]
fn without_udisks_no_drives_are_listed() {
    let fixture = Fixture::new("none");
    zbus::block_on(fixture.udisks.release_name(UDISKS)).unwrap();
    let drives = fixture.start();
    // Nothing differs from the empty list it starts with, so nothing is sent.
    nova_test_support::wait_until(WAIT, "udisks2 is followed", || {
        std::fs::read_to_string(fixture.dir.join("logs/nova.log"))
            .is_ok_and(|log| log.contains("udisks2 isn't running"))
    });
    assert!(lock(&drives.shared.state).listed.is_empty());
}

#[test]
fn a_stopped_module_ejects_nothing() {
    let fixture = Fixture::new("stopped");
    fixture.plug();
    let drives = fixture.start();
    fixture.next_where(|drives| drives.len() == 3);
    drives.mount("sdb1").unwrap();
    fixture.next_where(|drives| drives[2].get("mount").is_some());
    drives.stop();
    assert!(drives.eject("sdb1").unwrap_err().contains("stopped"));
    assert!(lock(&fixture.model).powered_off.is_empty());
}
