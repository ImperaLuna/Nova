//! The drives in Nova Files' sidebar, from udisks2 on the system bus
//! ([`Event::FilesDrives`]), and mounting and ejecting them (`files_mount`,
//! `files_eject`).
//!
//! Event-driven, on a thread of its own: udisks2's object manager says
//! when objects come and go (`InterfacesAdded`, `InterfacesRemoved`),
//! `PropertiesChanged` under its path when one changes (a mount, a label),
//! and `NameOwnerChanged` when udisksd starts or stops (everything is read
//! again, or emptied). They're followed before the objects are read, so
//! nothing that changes in between is missed. Which filesystems are listed
//! and what they're called is `drives.rs`; how full each is comes from
//! `statvfs` on its mount, read again whenever udisks2 says something
//! changed and when a window opens a folder ([`Drives::refresh`]). The
//! list is sent only when it changed, under the lock that holds it, so
//! clients never get an older one after a newer.
//!
//! Ejecting is Windows': every mounted filesystem on the drive is unmounted
//! (udisks2 writes what's left first), then the drive is powered off, and
//! a "Safe To Remove Hardware" notification says it can be pulled. When
//! something still has files open on it, udisks2 refuses with `DeviceBusy`
//! and it's answered as [`EjectOutcome::Busy`]: it stays mounted.

use crate::files::drives::{self, Block, DriveInfo, Known};
use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::notifications::{Notifications, Own};
use crate::session_bus::{Bus, Stop};
use futures_core::Stream;
use nova_protocol::{Drive, EjectOutcome, Event, LogLevel};
use std::collections::HashMap;
use std::ffi::CString;
use std::future::poll_fn;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::Poll;
use std::time::Duration;
use zbus::wire::{ObjectPath, OwnedObjectPath, OwnedVariant, Variant};
use zbus::{Connection, MatchRule, MessageStream};

const LOG_SOURCE: &str = "files";
pub const UDISKS: &str = "org.freedesktop.UDisks2";
pub const UDISKS_PATH: &str = "/org/freedesktop/UDisks2";
/// Where udisks2's block devices are, each named after its device.
pub const BLOCKS_PATH: &str = "/org/freedesktop/UDisks2/block_devices";
pub const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
pub const BLOCK: &str = "org.freedesktop.UDisks2.Block";
pub const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
pub const PARTITION: &str = "org.freedesktop.UDisks2.Partition";
pub const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
/// What udisks2 answers an unmount while files on it are open.
pub const BUSY_ERROR: &str = "org.freedesktop.UDisks2.Error.DeviceBusy";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// How long novad waits for the bus or udisksd to answer a read.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a mount, an unmount or a power-off may take: an unmount first
/// writes what's still waiting, which on a slow stick can take minutes.
const LONG_CALL_TIMEOUT: Duration = Duration::from_secs(600);
/// Who the "Safe To Remove Hardware" notification reads as from.
const NOTIFICATION_APP: &str = "Nova Files";

/// udisks2's objects as `GetManagedObjects` names them: by path, then by
/// interface, their properties.
type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedVariant>>>;

/// The sidebar's drives.
pub struct Drives {
    shared: Arc<Shared>,
    stop: Arc<Stop>,
}

struct Shared {
    bus: Bus,
    home: Option<PathBuf>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    state: Mutex<State>,
    /// Where "Safe To Remove Hardware" goes, while notifications are on.
    notifications: Mutex<Weak<Notifications>>,
}

#[derive(Default)]
struct State {
    known: Known,
    /// Counts changes to `known`, so a list worked out from an older one
    /// isn't sent.
    generation: u64,
    /// The list last sent.
    listed: Vec<Drive>,
    /// The connection for mounting and ejecting, made on first use.
    calls: Option<Connection>,
    stopped: bool,
}

impl Drives {
    /// Follows udisks2 on `bus` from a thread of its own; `home` is where
    /// mounts in the home folder are.
    pub fn start(bus: Bus, home: Option<PathBuf>, hub: Arc<Hub>, logger: Arc<Logger>) -> Self {
        let shared = Arc::new(Shared {
            bus,
            home,
            hub,
            logger,
            state: Mutex::default(),
            notifications: Mutex::new(Weak::new()),
        });
        let stop = Arc::new(Stop::default());
        let (following, stopping) = (Arc::clone(&shared), Arc::clone(&stop));
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(follow(&following, &stopping)) {
                following.logger.info(LOG_SOURCE, &format!("no drives from udisks2: {e}"));
                following.known(Known::default());
            }
        });
        Self { shared, stop }
    }

    /// Sends client `client` the drives as they are now.
    pub fn send_state(&self, client: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(client, &Event::FilesDrives { drives: state.listed.clone() });
    }

    /// Reads how full the mounted drives are again, and sends the list if
    /// that changed it. On a thread of its own, so a drive slow to answer
    /// holds up nobody.
    pub fn refresh(&self) {
        let shared = Arc::clone(&self.shared);
        std::thread::spawn(move || shared.list());
    }

    /// Where "Safe To Remove Hardware" goes: `notifications`, or nowhere.
    pub fn set_notifications(&self, notifications: Option<&Arc<Notifications>>) {
        *lock(&self.shared.notifications) = notifications.map(Arc::downgrade).unwrap_or_default();
    }

    /// Mounts drive `id` unless it is, and answers where it's mounted.
    pub fn mount(&self, id: &str) -> Result<String, String> {
        let drive = self.shared.listed(id)?;
        if let Some(mount) = drive.mount {
            return Ok(mount);
        }
        let calls = self.shared.calls()?;
        let options: HashMap<&str, Variant> = HashMap::new();
        let reply = zbus::block_on(calls.call_method(
            Some(UDISKS),
            block_path(id).as_str(),
            Some(FILESYSTEM),
            "Mount",
            &(options,),
        ))
        .and_then(|reply| reply.body().deserialize::<String>())
        .map_err(|e| format!("couldn't mount {}: {}", drive.name, error_text(&e)))?;
        self.shared.logger.info(LOG_SOURCE, &format!("mounted {} at {reply}", drive.name));
        self.shared.list();
        Ok(reply)
    }

    /// Ejects removable drive `id`: unmounts every filesystem on its device,
    /// then powers it off, or answers that it's busy.
    pub fn eject(&self, id: &str) -> Result<EjectOutcome, String> {
        let drive = self.shared.listed(id)?;
        if !drive.removable {
            return Err(format!("{} can't be ejected", drive.name));
        }
        let (device, info, mounted) = {
            let state = lock(&self.shared.state);
            let (device, info) = state.known.drive_of(id).ok_or_else(|| format!("{} has no drive", drive.name))?;
            let mounted: Vec<String> =
                state.known.mounted_on(device).into_iter().map(|(id, _)| id.to_string()).collect();
            (device.to_string(), info.clone(), mounted)
        };
        let calls = self.shared.calls()?;
        for filesystem in &mounted {
            let options: HashMap<&str, Variant> = HashMap::new();
            let unmounted = zbus::block_on(calls.call_method(
                Some(UDISKS),
                block_path(filesystem).as_str(),
                Some(FILESYSTEM),
                "Unmount",
                &(options,),
            ));
            match unmounted {
                Ok(_) => {}
                Err(e) if is_busy(&e) => {
                    self.shared.logger.info(LOG_SOURCE, &format!("{} is in use: {}", drive.name, error_text(&e)));
                    self.shared.list();
                    return Ok(EjectOutcome::Busy);
                }
                Err(e) => {
                    self.shared.list();
                    return Err(format!("couldn't unmount {}: {}", drive.name, error_text(&e)));
                }
            }
        }
        // Unmounted is safe to pull; powering off also makes the stick's
        // light go out, where it can.
        let method = if info.can_power_off { Some("PowerOff") } else { info.ejectable.then_some("Eject") };
        if let Some(method) = method {
            let options: HashMap<&str, Variant> = HashMap::new();
            if let Err(e) =
                zbus::block_on(calls.call_method(Some(UDISKS), device.as_str(), Some(DRIVE), method, &(options,)))
            {
                self.shared
                    .logger
                    .warn(LOG_SOURCE, &format!("{} unmounted, but {method} failed: {}", drive.name, error_text(&e)));
            }
        }
        self.shared.logger.info(LOG_SOURCE, &format!("ejected {}", drive.name));
        self.shared.list();
        if let Some(notifications) = lock(&self.shared.notifications).upgrade() {
            notifications.send_own(safe_to_remove(&drive.name));
        }
        Ok(EjectOutcome::Ejected)
    }

    /// Stops following udisks2: no list is sent after this.
    pub fn stop(&self) {
        lock(&self.shared.state).stopped = true;
        self.stop.stop();
    }
}

/// The notification an ejected drive gets, in Windows' words.
pub fn safe_to_remove(name: &str) -> Own {
    Own {
        app: NOTIFICATION_APP.into(),
        summary: "Safe To Remove Hardware".into(),
        body: format!("The \u{2018}{name}\u{2019} device can now be safely removed from the computer."),
        critical: false,
        replaces: 0,
        device: None,
    }
}

impl Shared {
    /// The drive `id`, while it's listed.
    fn listed(&self, id: &str) -> Result<Drive, String> {
        let state = lock(&self.state);
        state.listed.iter().find(|drive| drive.id == id).cloned().ok_or_else(|| format!("no drive {id}"))
    }

    /// The connection for long calls, made on first use.
    fn calls(&self) -> Result<Connection, String> {
        if let Some(calls) = lock(&self.state).calls.clone() {
            return Ok(calls);
        }
        let calls = zbus::block_on(self.bus.builder().method_timeout(LONG_CALL_TIMEOUT).build())
            .map_err(|e| format!("can't reach udisks2: {e}"))?;
        Ok(lock(&self.state).calls.get_or_insert(calls).clone())
    }

    /// udisks2 now knows `known`: lists it.
    fn known(&self, known: Known) {
        {
            let mut state = lock(&self.state);
            if state.known == known {
                return;
            }
            state.known = known;
            state.generation += 1;
        }
        self.list();
    }

    /// Works out the list from what udisks2 knows and how full each mount
    /// is, and sends it if it changed. `statvfs` runs outside the lock; a
    /// list worked out from what changed meanwhile is dropped, as the
    /// change works out its own.
    fn list(&self) {
        let (known, generation) = {
            let state = lock(&self.state);
            (state.known.clone(), state.generation)
        };
        let listed = drives::listed(&known, self.home.as_deref(), usage);
        let mut state = lock(&self.state);
        if state.stopped || state.generation != generation || state.listed == listed {
            return;
        }
        state.listed = listed;
        self.hub.publish(&Event::FilesDrives { drives: state.listed.clone() });
    }
}

/// How big the filesystem mounted at `mount` is and how much of it is
/// used, in bytes.
fn usage(mount: &str) -> Option<(u64, u64)> {
    let path = CString::new(Path::new(mount).as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid C string and `stats` is written in full
    // when the call succeeds.
    let stats = unsafe {
        if libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) != 0 {
            return None;
        }
        stats.assume_init()
    };
    let unit = stats.f_frsize;
    let size = stats.f_blocks.saturating_mul(unit);
    let used = stats.f_blocks.saturating_sub(stats.f_bfree).saturating_mul(unit);
    Some((size, used))
}

/// Block device `id`'s object.
fn block_path(id: &str) -> String {
    format!("{BLOCKS_PATH}/{id}")
}

fn is_busy(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, ..) if name.as_str() == BUSY_ERROR)
}

/// What udisks2 said went wrong, without the error's name.
fn error_text(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(name, text, _) => text.clone().unwrap_or_else(|| name.to_string()),
        other => other.to_string(),
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Owner,
    Objects,
    Changed,
}

/// Follows udisks2's objects on its bus until `stop` or the bus goes.
async fn follow(shared: &Shared, stop: &Stop) -> zbus::Result<()> {
    let connection = shared.bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
    let signal =
        |interface: &'static str| MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface);
    let owner_rule = signal(DBUS_NAME).member("NameOwnerChanged").sender(DBUS_NAME).arg(0, UDISKS).build()?;
    let mut owners = MessageStream::for_match_rule(owner_rule, &connection, None).await?;
    // Objects coming and going in one stream, so a stick pulled and pushed
    // back in while a call waits is handled in that order.
    let objects_rule = signal(OBJECT_MANAGER).path(UDISKS_PATH).build()?;
    let mut comings = MessageStream::for_match_rule(objects_rule, &connection, None).await?;
    let changed_rule = signal(PROPERTIES).member("PropertiesChanged").path_namespace(UDISKS_PATH).build()?;
    let mut changes = MessageStream::for_match_rule(changed_rule, &connection, None).await?;

    // udisksd starts on demand (D-Bus activation): ask for it.
    if let Err(e) =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "StartServiceByName", &(UDISKS, 0u32)).await
    {
        shared.logger.info(LOG_SOURCE, &format!("couldn't start udisks2: {e}"));
    }
    let mut owner = name_owner(&connection).await;
    let mut objects = match owner {
        Some(_) => managed_objects(&connection, shared).await,
        None => {
            shared.logger.info(LOG_SOURCE, "udisks2 isn't running; following it in case it starts");
            Objects::new()
        }
    };
    shared.known(known(&objects));

    loop {
        let next = poll_fn(|cx| {
            if stop.poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            for (kind, stream) in
                [(Signal::Owner, &mut owners), (Signal::Objects, &mut comings), (Signal::Changed, &mut changes)]
            {
                if let Poll::Ready(message) = Pin::new(stream).poll_next(cx) {
                    return Poll::Ready(message.map(|message| (kind, message)));
                }
            }
            Poll::Pending
        })
        .await;
        let Some((kind, message)) = next else { return Ok(()) };
        let message = message?;
        let sender = message.header().sender().map(|sender| sender.to_string());
        let from_udisks = sender.is_some() && sender == owner;
        match kind {
            Signal::Owner => {
                let Ok((_name, _old, new)) = message.body().deserialize::<(String, String, String)>() else { continue };
                if new.is_empty() {
                    shared.logger.info(LOG_SOURCE, "udisks2 stopped");
                    owner = None;
                    objects.clear();
                } else {
                    shared.logger.info(LOG_SOURCE, "udisks2 started");
                    owner = Some(new);
                    objects = managed_objects(&connection, shared).await;
                }
            }
            Signal::Objects if from_udisks => {
                let member = message.header().member().map(|member| member.to_string());
                match member.as_deref() {
                    Some("InterfacesAdded") => {
                        type Added = (OwnedObjectPath, HashMap<String, HashMap<String, OwnedVariant>>);
                        let Ok((path, interfaces)) = message.body().deserialize::<Added>() else { continue };
                        objects.entry(path).or_default().extend(interfaces);
                    }
                    Some("InterfacesRemoved") => {
                        let Ok((path, interfaces)) = message.body().deserialize::<(OwnedObjectPath, Vec<String>)>()
                        else {
                            continue;
                        };
                        if let Some(object) = objects.get_mut(&path) {
                            object.retain(|interface, _| !interfaces.contains(interface));
                            if object.is_empty() {
                                objects.remove(&path);
                            }
                        }
                    }
                    _ => continue,
                }
            }
            Signal::Changed if from_udisks => {
                let Some(path) = message.header().path().map(|path| OwnedObjectPath::from(path.to_owned())) else {
                    continue;
                };
                type Changed = (String, HashMap<String, OwnedVariant>, Vec<String>);
                let Ok((interface, changed, invalidated)) = message.body().deserialize::<Changed>() else { continue };
                let Some(properties) = objects.get_mut(&path).and_then(|object| object.get_mut(&interface)) else {
                    continue;
                };
                properties.extend(changed);
                if !invalidated.is_empty() {
                    match get_all(&connection, &path, &interface).await {
                        Ok(read) => *properties = read,
                        Err(e) => shared.logger.log(LogLevel::Debug, LOG_SOURCE, &format!("can't read {path}: {e}")),
                    }
                }
            }
            Signal::Objects | Signal::Changed => continue,
        }
        shared.known(known(&objects));
    }
}

/// udisksd's unique name, `None` while it doesn't run.
async fn name_owner(connection: &Connection) -> Option<String> {
    let reply =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(UDISKS,)).await.ok()?;
    reply.body().deserialize().ok()
}

/// Every object udisks2 has, empty when it can't say.
async fn managed_objects(connection: &Connection, shared: &Shared) -> Objects {
    let reply = connection
        .call_method(Some(UDISKS), UDISKS_PATH, Some(OBJECT_MANAGER), "GetManagedObjects", &())
        .await
        .and_then(|reply| reply.body().deserialize::<Objects>());
    reply.unwrap_or_else(|e| {
        shared.logger.warn(LOG_SOURCE, &format!("can't list udisks2's drives: {e}"));
        Objects::new()
    })
}

async fn get_all(
    connection: &Connection,
    path: &OwnedObjectPath,
    interface: &str,
) -> zbus::Result<HashMap<String, OwnedVariant>> {
    connection
        .call_method(Some(UDISKS), path.as_str(), Some(PROPERTIES), "GetAll", &(interface,))
        .await?
        .body()
        .deserialize()
}

/// What udisks2's `objects` say about its block devices and drives.
fn known(objects: &Objects) -> Known {
    let mut known = Known::default();
    for (path, interfaces) in objects {
        if let Some(drive) = interfaces.get(DRIVE) {
            known.drives.insert(path.to_string(), drive_info(drive));
        }
        let Some(block) = interfaces.get(BLOCK) else { continue };
        let Some(id) = path.as_str().strip_prefix(BLOCKS_PATH).and_then(|rest| rest.strip_prefix('/')) else {
            continue;
        };
        let filesystem = interfaces.get(FILESYSTEM);
        let partition = interfaces.get(PARTITION);
        known.blocks.insert(
            id.to_string(),
            Block {
                drive: object(block, "Drive"),
                size: number(block, "Size"),
                label: text(block, "IdLabel"),
                hint_name: text(block, "HintName"),
                hint_ignore: flag(block, "HintIgnore"),
                hint_system: flag(block, "HintSystem"),
                mounts: filesystem.map(mount_points),
                partition_type: partition.map(|partition| text(partition, "Type")),
            },
        );
    }
    known
}

fn drive_info(drive: &HashMap<String, OwnedVariant>) -> DriveInfo {
    DriveInfo {
        vendor: text(drive, "Vendor"),
        model: text(drive, "Model"),
        removable: flag(drive, "Removable"),
        connection_bus: text(drive, "ConnectionBus"),
        can_power_off: flag(drive, "CanPowerOff"),
        ejectable: flag(drive, "Ejectable"),
    }
}

fn text(properties: &HashMap<String, OwnedVariant>, name: &str) -> String {
    properties.get(name).and_then(|value| <&str>::try_from(value).ok()).unwrap_or_default().to_string()
}

fn object(properties: &HashMap<String, OwnedVariant>, name: &str) -> String {
    properties
        .get(name)
        .and_then(|value| <&ObjectPath>::try_from(value).ok())
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn number(properties: &HashMap<String, OwnedVariant>, name: &str) -> u64 {
    properties.get(name).and_then(|value| u64::try_from(value).ok()).unwrap_or(0)
}

fn flag(properties: &HashMap<String, OwnedVariant>, name: &str) -> bool {
    properties.get(name).and_then(|value| bool::try_from(value).ok()).unwrap_or(false)
}

/// `MountPoints`: byte strings ending in a NUL. One that isn't UTF-8 is
/// left out, since the UI couldn't open it by name.
fn mount_points(filesystem: &HashMap<String, OwnedVariant>) -> Vec<String> {
    let points: Vec<Vec<u8>> = filesystem
        .get("MountPoints")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| value.try_into().ok())
        .unwrap_or_default();
    points
        .into_iter()
        .filter_map(|mut point| {
            if point.last() == Some(&0) {
                point.pop();
            }
            String::from_utf8(point).ok()
        })
        .collect()
}

#[cfg(test)]
#[path = "udisks_tests.rs"]
mod tests;
