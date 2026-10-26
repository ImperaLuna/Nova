//! The list of apps in the system tray (feature `tray`).
//!
//! Apps show tray icons as StatusNotifierItems on the session bus and
//! register them with a watcher, `org.kde.StatusNotifierWatcher`. novad is
//! that watcher, so the list lives as long as novad and survives UI
//! restarts; the UI (Quickshell's `SystemTray`) is the host that reads each
//! item's icon and menu. Owned here rather than by the UI because Chromium
//! and Electron look for a host once, at start: an app autostarted before
//! the UI would otherwise fall back to an X11 icon for good. So the watcher
//! always says a host is there, and novad claims the name before it starts
//! the UI.
//!
//! Each item is published as an [`Event::Tray`] once its id and title are
//! read, with the desktop app it belongs to (see `app`), for the launcher
//! to reopen an app that only sits in the tray. Items go when the bus name
//! they're on goes. The tray takes at most [`MAX_ITEMS`] items, and checks
//! what apps send it, since anything on the session bus can register:
//! an app registers only items on its own connection, a few at most, and
//! an item its app says isn't there is dropped.

mod app;
pub use app::BRIDGE;
#[cfg(test)]
pub mod tests;

use crate::hub::{ClientId, Hub};
use crate::launcher::Launcher;
use crate::logger::Logger;
use crate::session_bus::{self, Bus, Stop};
use crate::{lock, state_file};
use futures_core::Stream;
use nova_protocol::{App, Event, LogLevel, Tray as TrayEvent, TrayItem};
use std::future::poll_fn;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use zbus::fdo::RequestNameReply;
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::wire::{ObjectPath, OwnedVariant};
use zbus::{Connection, MatchRule, MessageStream, interface};

pub const FEATURE_ID: &str = "tray";
const LOG_SOURCE: &str = "tray";

pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// Where an item is when an app registers only its bus name.
const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// More than any desktop has; past it, registrations are refused.
pub const MAX_ITEMS: usize = 64;
/// More than any one app puts up; past it, that app's registrations are refused.
const MAX_ITEMS_PER_APP: usize = 8;
/// How many apps can be hidden at once.
const MAX_HIDDEN: usize = MAX_ITEMS;
/// The longest service string a registration may send. Bus names are at
/// most 255 bytes; this leaves room for a path after one.
const MAX_SERVICE_LEN: usize = 512;
/// Ids and titles are cut to this many characters.
const MAX_TEXT_CHARS: usize = 256;
/// How long novad waits for an app, or the bus, to answer a call.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Where the tray's watcher is, and what it keeps.
#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    /// The apps hidden from the bar's tray.
    pub state_file: PathBuf,
}

impl Sources {
    pub fn new(state_dir: &std::path::Path) -> Self {
        Self { bus: Bus::Session, state_file: state_dir.join("tray.json") }
    }
}

pub struct Tray {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    /// Where an item's process is looked up (`/proc`, or a copy in tests).
    proc_root: PathBuf,
    state_file: PathBuf,
    /// Finds the app an item belongs to, while the `launcher` feature runs.
    launcher: Mutex<Option<Arc<Launcher>>>,
    /// The watcher's connection, once it's up, for `activate_app`.
    connection: Mutex<Option<Connection>>,
    stop: Stop,
}

#[derive(Default)]
struct State {
    items: Vec<Item>,
    saved: Saved,
    /// The last event published, so an unchanged list isn't sent again.
    sent: Option<TrayEvent>,
}

/// What survives a restart.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Saved {
    #[serde(default)]
    hidden: Vec<String>,
}

struct Item {
    /// `<bus name><object path>`, as the spec has hosts read it.
    address: String,
    bus: String,
    /// The unique name that owned `bus` at registration: the item goes when it does.
    owner: String,
    /// `None` until its id and title are read.
    read: Option<Read>,
}

struct Read {
    id: String,
    title: String,
    /// Names to look its app up by (see `app`).
    app_ids: Vec<String>,
    /// Put up by the X11 icon bridge (see `app::is_bridge`).
    bridged: bool,
}

impl Tray {
    /// Claims the watcher's name on `bus` and serves it on a thread of its
    /// own. Returns once the name is claimed, queued behind another
    /// watcher, or connecting failed (logged), so apps started after it
    /// find the watcher.
    pub fn start(
        sources: Sources,
        proc_root: PathBuf,
        launcher: Option<Arc<Launcher>>,
        hub: Arc<Hub>,
        logger: Arc<Logger>,
    ) -> Arc<Self> {
        let Sources { bus, state_file } = sources;
        let saved = match std::fs::read_to_string(&state_file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", state_file.display()));
                Saved::default()
            }),
            Err(_) => Saved::default(),
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State { saved, ..State::default() }),
            hub,
            logger,
            proc_root,
            state_file,
            launcher: Mutex::new(launcher),
            connection: Mutex::new(None),
            stop: Stop::default(),
        });
        let (ready, started) = std::sync::mpsc::channel();
        let serving = Arc::clone(&shared);
        std::thread::spawn(move || {
            let result = zbus::block_on(serve(&serving, bus, &ready));
            if let Err(e) = result {
                serving.logger.warn(LOG_SOURCE, &format!("tray icons won't show: {e}"));
            }
            let _ = ready.send(());
        });
        // Waits as long as connecting may take; a timed-out call fails sooner.
        let _ = started.recv_timeout(2 * CALL_TIMEOUT);
        Arc::new(Self { shared })
    }

    /// Lets go of the watcher's name and publishes nothing more.
    pub fn stop(&self) {
        let _state = lock(&self.shared.state);
        self.shared.stop.stop();
    }

    /// Looks apps up with `launcher` from now on (`None` while the launcher
    /// is off), and publishes the items with their apps again.
    pub fn set_launcher(&self, launcher: Option<Arc<Launcher>>) {
        *lock(&self.shared.launcher) = launcher;
        self.shared.publish(&mut lock(&self.shared.state));
    }

    /// Looks the items' apps up again, after the launcher's apps changed.
    pub fn apps_changed(&self) {
        self.shared.publish(&mut lock(&self.shared.state));
    }

    /// Hides the app `id` (an item's app id, or its own id without one)
    /// from the bar's tray, or shows it again, and remembers that. Only an
    /// app in the tray now can be hidden.
    pub fn hide(&self, id: &str, hidden: bool) -> Result<(), String> {
        let mut state = lock(&self.shared.state);
        let at = state.saved.hidden.iter().position(|kept| kept == id);
        match (hidden, at) {
            (true, None) => {
                if !self.shared.event(&state).items.iter().any(|item| key(item) == id) {
                    return Err(format!("no tray app {id:?}"));
                }
                if state.saved.hidden.len() >= MAX_HIDDEN {
                    return Err(format!("at most {MAX_HIDDEN} tray apps can be hidden"));
                }
                state.saved.hidden.push(id.to_string());
            }
            (false, Some(at)) => {
                state.saved.hidden.remove(at);
            }
            (true, Some(_)) | (false, None) => return Ok(()),
        }
        // Under the lock, so changes reach the file in the order they were made.
        if let Err(e) = state_file::save(&self.shared.state_file, &state.saved) {
            let message = format!("can't save {}: {e}", self.shared.state_file.display());
            self.shared.logger.warn(LOG_SOURCE, &message);
        }
        self.shared.publish(&mut state);
        Ok(())
    }

    /// Asks the tray item of the app `app_id` (a desktop entry id) to show
    /// its window, as a click on its icon does (`Activate`). False when no
    /// item belongs to that app. For the launcher, to reopen an app that
    /// only sits in the tray.
    pub fn activate_app(&self, app_id: &str) -> Result<bool, String> {
        let item = self.item(|item| item.app.as_ref().is_some_and(|app| app.id == app_id));
        let Some(item) = item else { return Ok(false) };
        self.send_activate(&item.address).map_err(|e| format!("{app_id}'s tray icon didn't answer: {e}"))
    }

    /// Asks the item at `address` to show its window, as a click on its
    /// icon does (`Activate`), and says which installed app it belongs to.
    pub fn activate(&self, address: &str) -> Result<Option<String>, String> {
        let item = self.item(|item| item.address == address).ok_or_else(|| format!("no tray icon at {address}"))?;
        if !self.send_activate(address).map_err(|e| format!("{}'s tray icon didn't answer: {e}", item.id))? {
            return Err("the tray isn't on the session bus".into());
        }
        Ok(item.app.map(|app| app.id))
    }

    /// The first item published (one whose id and title were read) that
    /// `wanted` picks.
    fn item(&self, wanted: impl Fn(&TrayItem) -> bool) -> Option<TrayItem> {
        let state = lock(&self.shared.state);
        self.shared.event(&state).items.into_iter().find(|item| wanted(item))
    }

    /// Calls `Activate` on the item at `address`; false while the tray isn't
    /// on the bus.
    fn send_activate(&self, address: &str) -> Result<bool, String> {
        let Some(connection) = lock(&self.shared.connection).clone() else {
            return Ok(false);
        };
        let slash = address.find('/').ok_or_else(|| format!("bad tray address {address}"))?;
        let (bus, path) = address.split_at(slash);
        zbus::block_on(connection.call_method(Some(bus), path, Some(ITEM_INTERFACE), "Activate", &(0i32, 0i32)))
            .map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Sends a new subscriber, client `id`, the items as they are, under the
    /// lock that publishing takes, so a newer list can't arrive before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Tray(self.shared.event(&state)));
    }
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.stopped()
    }

    fn event(&self, state: &State) -> TrayEvent {
        let launcher = lock(&self.launcher).clone();
        let read: Vec<(bool, TrayItem)> = state
            .items
            .iter()
            .filter_map(|item| {
                let read = item.read.as_ref()?;
                Some((
                    read.bridged,
                    TrayItem {
                        address: item.address.clone(),
                        id: read.id.clone(),
                        title: read.title.clone(),
                        app: launcher.as_ref().and_then(|launcher| launcher.find_app(&read.app_ids)),
                    },
                ))
            })
            .collect();
        let items = without_bridged_twins(read);
        TrayEvent { items, hidden: state.saved.hidden.clone() }
    }

    fn publish(&self, state: &mut State) {
        if self.stopped() {
            return;
        }
        let event = self.event(state);
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Tray(event.clone()));
            state.sent = Some(event);
        }
    }

    /// Takes `service` from the app on `sender`, as `RegisterStatusNotifierItem` sends it:
    /// a bus name, or an object path on the sender's connection. Gives the
    /// item's address, and whether it's new rather than registered again.
    async fn register(&self, connection: &Connection, sender: &str, service: &str) -> Result<(String, bool), String> {
        if service.len() > MAX_SERVICE_LEN {
            return Err(format!("the service is longer than {MAX_SERVICE_LEN} bytes"));
        }
        let (bus, path) = match service.find('/') {
            Some(0) => (sender, service),
            Some(slash) => service.split_at(slash),
            None => (service, DEFAULT_ITEM_PATH),
        };
        BusName::try_from(bus).map_err(|e| format!("{bus} isn't a bus name: {e}"))?;
        ObjectPath::try_from(path).map_err(|e| format!("{path} isn't an object path: {e}"))?;
        let owner = match UniqueName::try_from(bus) {
            Ok(_) => bus.to_string(),
            Err(_) => name_owner(connection, bus).await.map_err(|e| format!("{bus} has no owner: {e}"))?,
        };
        // Else anything could fill the tray with names others own (novad's, say).
        if owner != sender {
            return Err(format!("{bus} isn't the sender's"));
        }
        let address = format!("{bus}{path}");
        let mut state = lock(&self.state);
        if self.stopped() {
            return Err("the tray is stopping".into());
        }
        if state.items.iter().any(|item| item.address == address) {
            return Ok((address, false));
        }
        if state.items.len() >= MAX_ITEMS {
            return Err(format!("the tray already has {MAX_ITEMS} items"));
        }
        if state.items.iter().filter(|item| item.owner == owner).count() >= MAX_ITEMS_PER_APP {
            return Err(format!("the sender already has {MAX_ITEMS_PER_APP} items"));
        }
        state.items.push(Item { address: address.clone(), bus: bus.to_string(), owner, read: None });
        Ok((address, true))
    }

    /// Reads the id and title of the item at `address`, and which process
    /// it's in, then publishes it. An item whose app has gone meanwhile, or
    /// that its app says isn't there, is dropped; one that answers wrongly
    /// shows with what could be read. It
    /// asks the connection that registered it, and keeps the answers only
    /// while that registration stands (a name handed over meanwhile is
    /// another app's).
    async fn read(&self, connection: &Connection, address: &str) {
        let Some((bus, owner)) = lock(&self.state)
            .items
            .iter()
            .find(|item| item.address == address)
            .map(|item| (item.bus.clone(), item.owner.clone()))
        else {
            return;
        };
        let path = &address[bus.len()..];
        let id = match item_text(connection, &owner, path, "Id").await {
            Ok(id) => id,
            // The app says there's no such item; a slow one stays, without an id.
            Err(e) if says_not_there(&e) => {
                self.logger.info(LOG_SOURCE, &format!("dropped {address}: can't read its id: {e}"));
                unregister(connection, self.drop_items(|item| item.address == address && item.owner == owner)).await;
                return;
            }
            Err(_) => String::new(),
        };
        let title = item_text(connection, &owner, path, "Title").await;
        let pid = connection
            .call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetConnectionUnixProcessID", &(owner.as_str(),))
            .await
            .and_then(|reply| reply.body().deserialize::<u32>());
        if let Err(e) = &pid {
            // A name with no owner has nothing left to show.
            if matches!(e, zbus::Error::MethodError(name, ..) if name.as_str() == "org.freedesktop.DBus.Error.NameHasNoOwner")
            {
                unregister(connection, self.drop_bus(&owner)).await;
                return;
            }
            let message = format!("can't tell which process {address} is in: {e}");
            self.logger.log(LogLevel::Debug, LOG_SOURCE, &message);
        }
        let title = title.unwrap_or_default();
        let app_ids = app::candidates(&self.proc_root, pid.as_ref().ok().copied(), &id);
        let bridged = pid.as_ref().is_ok_and(|&pid| app::is_bridge(&self.proc_root, pid));
        let mut state = lock(&self.state);
        let Some(item) = state.items.iter_mut().find(|item| item.address == address && item.owner == owner) else {
            return;
        };
        self.logger.info(LOG_SOURCE, &format!("{address} came: {id} ({})", app_ids.join(", ")));
        item.read = Some(Read { id, title, app_ids, bridged });
        self.publish(&mut state);
    }

    /// Drops the items on `name`, a bus name that lost its owner, and says
    /// which went.
    fn drop_bus(&self, name: &str) -> Vec<String> {
        self.drop_items(|item| item.bus == name || item.owner == name)
    }

    /// Drops the items `old` registered under the well-known `name`, which
    /// went to another connection: its app registers them again.
    fn drop_handed_over(&self, name: &str, old: &str) -> Vec<String> {
        self.drop_items(|item| item.bus == name && item.owner == old)
    }

    fn drop_items(&self, gone: impl Fn(&Item) -> bool) -> Vec<String> {
        let mut state = lock(&self.state);
        let (gone, kept) = std::mem::take(&mut state.items).into_iter().partition::<Vec<_>, _>(gone);
        state.items = kept;
        let gone: Vec<String> = gone.into_iter().map(|item| item.address).collect();
        if !gone.is_empty() {
            self.logger.info(LOG_SOURCE, &format!("{} went", gone.join(", ")));
            self.publish(&mut state);
        }
        gone
    }

    /// The registered items' addresses, as the watcher lists them.
    fn addresses(&self) -> Vec<String> {
        lock(&self.state).items.iter().map(|item| item.address.clone()).collect()
    }
}

/// The items, less those put up by the X11 icon bridge (`true`) for an app
/// that also put up a real item: Steam does both.
fn without_bridged_twins(items: Vec<(bool, TrayItem)>) -> Vec<TrayItem> {
    let has_own_item = |app: &App| {
        items.iter().any(|(bridged, item)| !bridged && item.app.as_ref().is_some_and(|own| own.id == app.id))
    };
    let twins: Vec<bool> =
        items.iter().map(|(bridged, item)| *bridged && item.app.as_ref().is_some_and(has_own_item)).collect();
    items.into_iter().zip(twins).filter(|(_, twin)| !twin).map(|((_, item), _)| item).collect()
}

/// What `hide` and `Tray::hidden` know an item's app by.
fn key(item: &TrayItem) -> &str {
    item.app.as_ref().map_or(&item.id, |app| &app.id)
}

/// Connects to `bus`, claims the watcher's name and serves it, dropping
/// items whose app leaves the bus, until `stop` or the bus goes. Sends on
/// `ready` once apps can register.
async fn serve(shared: &Arc<Shared>, bus: Bus, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let builder = bus.builder();
    let watcher = Watcher { shared: Arc::clone(shared) };
    let connection = builder.method_timeout(CALL_TIMEOUT).serve_at(WATCHER_PATH, watcher).build().await?;
    // Followed before the name is claimed, so no app leaving in between is missed.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(DBUS_NAME)
        .interface(DBUS_NAME)
        .member("NameOwnerChanged")
        .build()?;
    let mut owners = MessageStream::for_match_rule(rule, &connection, None).await?;
    // Queued behind another watcher (another shell's, say), novad takes over when it leaves.
    match session_bus::claim(&connection, WATCHER_NAME).await? {
        RequestNameReply::InQueue => shared.logger.warn(
            LOG_SOURCE,
            &format!("another program owns {WATCHER_NAME}; tray icons show through it until it quits"),
        ),
        _ => shared.logger.info(LOG_SOURCE, &format!("watching for tray icons as {WATCHER_NAME}")),
    }
    *lock(&shared.connection) = Some(connection.clone());
    let _ = ready.send(());
    let result = follow_owners(shared, &connection, &mut owners).await;
    // The watcher it serves holds `shared`: kept, the connection would never
    // close, and a stopped tray would keep the name from the next one.
    *lock(&shared.connection) = None;
    result
}

/// Drops the items of apps that leave the bus, until `stop` or the bus goes.
async fn follow_owners(shared: &Shared, connection: &Connection, owners: &mut MessageStream) -> zbus::Result<()> {
    let emitter = SignalEmitter::new(connection, WATCHER_PATH)?;
    while let Some(message) = next_unless_stopped(owners, &shared.stop).await {
        let (name, old, new): (String, String, String) = message?.body().deserialize()?;
        let gone = match (old.is_empty(), new.is_empty()) {
            (_, true) => shared.drop_bus(&name),
            (false, false) => shared.drop_handed_over(&name, &old),
            (true, false) => continue,
        };
        for address in gone {
            emitter.status_notifier_item_unregistered(&address).await?;
        }
    }
    Ok(())
}

/// The stream's next message; `None` once the connection closes or `stop` is set.
async fn next_unless_stopped(stream: &mut MessageStream, stop: &Stop) -> Option<zbus::Result<zbus::Message>> {
    poll_fn(|cx| {
        if stop.poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Pin::new(&mut *stream).poll_next(cx)
    })
    .await
}

/// Whether `e` is an app's answer that it has no such item, rather than
/// no answer in time or a passing failure.
fn says_not_there(e: &zbus::Error) -> bool {
    use zbus::DBusError;
    let name = match e {
        zbus::Error::MethodError(name, ..) => name.to_string(),
        zbus::Error::FDO(e) => e.name().to_string(),
        _ => return false,
    };
    matches!(
        name.as_str(),
        "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownProperty"
    )
}

/// Tells hosts the items at `addresses` went.
async fn unregister(connection: &Connection, addresses: Vec<String>) {
    let Ok(emitter) = SignalEmitter::new(connection, WATCHER_PATH) else { return };
    for address in addresses {
        let _ = emitter.status_notifier_item_unregistered(&address).await;
    }
}

async fn name_owner(connection: &Connection, name: &str) -> zbus::Result<String> {
    let reply = connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(name,)).await?;
    reply.body().deserialize()
}

/// A string property of the item at `bus` and `path`, cut to [`MAX_TEXT_CHARS`].
async fn item_text(connection: &Connection, bus: &str, path: &str, name: &str) -> zbus::Result<String> {
    let reply = connection.call_method(Some(bus), path, Some(PROPERTIES), "Get", &(ITEM_INTERFACE, name)).await?;
    let text: String = reply.body().deserialize::<OwnedVariant>()?.try_into()?;
    Ok(text.chars().take(MAX_TEXT_CHARS).collect())
}

/// The watcher apps see on the bus.
struct Watcher {
    shared: Arc<Shared>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header.sender().map(|sender| sender.to_string()).unwrap_or_default();
        let (address, new) = self.shared.register(connection, &sender, service).await.map_err(|e| {
            self.shared.logger.warn(LOG_SOURCE, &format!("refused a tray icon from {sender}: {e}"));
            zbus::fdo::Error::InvalidArgs(e)
        })?;
        emitter.status_notifier_item_registered(&address).await?;
        // Read once, by the first registration: an app registering again
        // doesn't get another thread.
        if !new {
            return Ok(());
        }
        // On a thread of its own, so the app gets its answer first and a slow
        // one holds up nobody else.
        let shared = Arc::clone(&self.shared);
        let connection = connection.clone();
        std::thread::spawn(move || zbus::block_on(shared.read(&connection, &address)));
        Ok(())
    }

    /// Hosts are welcome, but the watcher says there's one whatever they do.
    async fn register_status_notifier_host(
        &self,
        _service: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        emitter.status_notifier_host_registered().await?;
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.shared.addresses()
    }

    /// Always: Nova's UI is the host, and it may still be starting.
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
