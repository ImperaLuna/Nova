//! The list of apps in the system tray (feature `tray`).
//!
//! Apps show tray icons as StatusNotifierItems on the session bus and
//! register them with a watcher, `org.kde.StatusNotifierWatcher`. novad is
//! that watcher, so the list lives as long as novad and survives UI
//! restarts; today's Quickshell UI (its `SystemTray`) is the host that
//! reads each item's icon and menu. The GPUI UI talks only to novad, so
//! novad reads them too (`look`, `menu`): each item's picture as a file,
//! whether it asks for attention, and its menu one level at a time. Only
//! reading: nothing registers as a host, and nothing is sent to an app
//! until the UI asks. Owned here rather than by the UI because Chromium
//! and Electron look for a host once, at start: an app autostarted before
//! the UI would otherwise fall back to an X11 icon for good. So the watcher
//! always says a host is there, and novad claims the name before it starts
//! the UI.
//!
//! Each item is published as an [`Event::Tray`] once its id and title are
//! read, with the desktop app it belongs to (see `app`, found in the app
//! index, `nova_apps`), for the launcher
//! to reopen an app that only sits in the tray. Items go when the bus name
//! they're on goes. The tray takes at most [`MAX_ITEMS`] items, and checks
//! what apps send it, since anything on the session bus can register:
//! an app registers only items on its own connection, a few at most, and
//! an item its app says isn't there is dropped.

mod app;
pub use app::candidates;
mod bus;
mod look;
mod menu;
use bus::*;
#[cfg(any(test, feature = "test-support"))]
#[expect(clippy::unwrap_used, reason = "test helpers fail the test that calls them, loudly")]
pub mod test_items;
#[cfg(test)]
mod tests;

use futures_core::Stream;
use nova_app_info::App;
use nova_apps::AppIndex;
use nova_base::{lock, state_file};
use nova_carrier::log::LogLevel;
use nova_hub::{ClientId, Hub};
use nova_icons::Icons;
use nova_log::Logger;
use nova_session_bus::{self as session_bus, Bus, Stop};
use nova_tray_api::{Event, Tray as TrayEvent, TrayItem, TrayMenuLevel};
use std::collections::HashMap;
use std::future::poll_fn;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use zbus::OwnedVariant as OwnedValue;
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
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";

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
    /// Where an item's process is looked up (`/proc`, or a copy in tests).
    pub proc_root: PathBuf,
    /// Where an item's icon name is looked up.
    pub icons: Arc<Icons>,
    /// Where the pictures items send as pixels are written, under the
    /// runtime dir. Emptied at start.
    pub pictures: PathBuf,
}

impl Sources {
    pub fn new(state_dir: &std::path::Path, runtime_dir: &std::path::Path, icons: Arc<Icons>) -> Self {
        Self {
            bus: Bus::Session,
            state_file: state_dir.join("tray.json"),
            proc_root: PathBuf::from(nova_process::procfs::PROC),
            icons,
            pictures: runtime_dir.join("tray"),
        }
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
    /// Finds the app an item belongs to.
    apps: Arc<AppIndex>,
    icons: Arc<Icons>,
    pictures: PathBuf,
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
    /// The number the next item gets, which its pictures are named by.
    next_serial: u64,
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
    serial: u64,
    /// `None` until its id and title are read.
    read: Option<Read>,
}

impl Item {
    fn path(&self) -> &str {
        &self.address[self.bus.len()..]
    }
}

/// What came of calling an item's `Activate`.
#[derive(PartialEq)]
enum Sent {
    Activated,
    /// The tray isn't on the bus (yet).
    NotOnBus,
    /// The item has no `Activate`.
    MenuOnly,
}

struct Read {
    id: String,
    title: String,
    /// Names to look its app up by (see `app`).
    app_ids: Vec<String>,
    /// Put up by the X11 icon bridge (see `app::is_bridge`).
    bridged: bool,
    /// It has no `Activate` (see [`TrayItem::menu_only`]).
    menu_only: bool,
    /// Its picture, status and menu.
    look: look::Look,
    /// Its menu's layout revision, the newest seen: a click on an entry
    /// read at an older one is refused.
    menu_revision: Option<u32>,
}

impl Tray {
    /// Claims the watcher's name on `bus` and serves it on a thread of its
    /// own. Returns once the name is claimed, queued behind another
    /// watcher, or connecting failed (logged), so apps started after it
    /// find the watcher.
    pub fn start(sources: Sources, apps: Arc<AppIndex>, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let Sources { bus, state_file, proc_root, icons, pictures } = sources;
        look::clear(&pictures);
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
            apps,
            icons,
            pictures,
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
        let tray = Arc::new(Self { shared });
        // Looked up again too, for a read of the apps that ended while it
        // connected, before it listened.
        tray.shared.apps.on_change(&tray, Self::apps_changed);
        tray.apps_changed();
        tray
    }

    /// Lets go of the watcher's name and publishes nothing more.
    pub fn stop(&self) {
        let _state = lock(&self.shared.state);
        self.shared.stop.stop();
    }

    /// Looks the items' apps up again, after the installed apps changed.
    fn apps_changed(&self) {
        let mut state = lock(&self.shared.state);
        // Before its first event the tray has no names to change.
        if state.sent.is_some() {
            self.shared.publish(&mut state);
        }
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

    /// Whether the app `app_id` (a desktop entry id) has a tray item: it
    /// runs, even with no window open.
    pub fn has_app(&self, app_id: &str) -> bool {
        self.item(|item| item.app.as_ref().is_some_and(|app| app.id == app_id)).is_some()
    }

    /// Asks the tray item of the app `app_id` (a desktop entry id) to show
    /// its window, as a click on its icon does (`Activate`). False when no
    /// item belongs to that app, or its item has no `Activate`. For the
    /// launcher, to reopen an app that only sits in the tray.
    pub fn activate_app(&self, app_id: &str) -> Result<bool, String> {
        let item = self.item(|item| item.app.as_ref().is_some_and(|app| app.id == app_id) && !item.menu_only);
        let Some(item) = item else { return Ok(false) };
        let sent = self.send_activate(&item.address).map_err(|e| format!("{app_id}'s tray icon didn't answer: {e}"))?;
        Ok(sent == Sent::Activated)
    }

    /// The installed app the item at `address` belongs to, if novad found it.
    pub fn app_at(&self, address: &str) -> Result<Option<String>, String> {
        let item = self.item(|item| item.address == address).ok_or_else(|| format!("no tray icon at {address}"))?;
        Ok(item.app.map(|app| app.id))
    }

    /// Asks the item at `address` to show its window, as a click on its
    /// icon does (`Activate`).
    pub fn activate(&self, address: &str) -> Result<(), String> {
        let item = self.item(|item| item.address == address).ok_or_else(|| format!("no tray icon at {address}"))?;
        match self.send_activate(address).map_err(|e| format!("{}'s tray icon didn't answer: {e}", item.id))? {
            Sent::Activated => Ok(()),
            Sent::NotOnBus => Err("the tray isn't on the session bus".into()),
            Sent::MenuOnly => Err(format!("{}'s tray icon has only a menu", item.id)),
        }
    }

    /// A middle click on the item at `address` (`SecondaryActivate`).
    pub fn secondary_activate(&self, address: &str) -> Result<(), String> {
        let (connection, owner, path) = self.reach(address)?;
        zbus::block_on(connection.call_method(
            Some(owner.as_str()),
            path.as_str(),
            Some(ITEM_INTERFACE),
            "SecondaryActivate",
            &(0i32, 0i32),
        ))
        .map(drop)
        .map_err(|e| format!("{address} didn't answer: {e}"))
    }

    /// The level under entry `id` of the menu of the item at `address`.
    pub fn menu_level(&self, address: &str, id: i32) -> Result<TrayMenuLevel, String> {
        let (connection, owner, path) = self.reach_menu(address)?;
        let level = zbus::block_on(menu::level(&connection, &owner, &path, id))
            .map_err(|e| format!("{address}'s menu didn't answer: {e}"))?;
        self.shared.saw_revision(&owner, &path, level.revision);
        Ok(level)
    }

    /// Clicks entry `id` of the menu of the item at `address`, read at
    /// `revision`: refused once the app changed its menu since.
    pub fn run_menu_entry(&self, address: &str, id: i32, revision: u32) -> Result<(), String> {
        let (connection, owner, path) = self.reach_menu(address)?;
        let newest = self.read_of(address, |read| read.menu_revision)?;
        // Never read (or read from a menu the item has since replaced): refused too.
        if newest != Some(revision) {
            return Err(format!("{address}'s menu changed since it was read"));
        }
        zbus::block_on(menu::click(&connection, &owner, &path, id))
            .map_err(|e| format!("{address}'s menu didn't answer: {e}"))
    }

    /// The connection, owner and object path to call the item at `address` on.
    fn reach(&self, address: &str) -> Result<(Connection, String, String), String> {
        let connection = lock(&self.shared.connection).clone().ok_or("the tray isn't on the session bus")?;
        let state = lock(&self.shared.state);
        let item = state
            .items
            .iter()
            .find(|item| item.address == address)
            .ok_or_else(|| format!("no tray icon at {address}"))?;
        Ok((connection, item.owner.clone(), item.path().to_string()))
    }

    /// Like `reach`, with the path of the item's menu.
    fn reach_menu(&self, address: &str) -> Result<(Connection, String, String), String> {
        let (connection, owner, _) = self.reach(address)?;
        let path = self
            .read_of(address, |read| read.look.menu_path.clone())?
            .ok_or_else(|| format!("{address} has no menu"))?;
        Ok((connection, owner, path))
    }

    /// What `pick` finds in what was read of the item at `address`.
    fn read_of<T>(&self, address: &str, pick: impl Fn(&Read) -> T) -> Result<T, String> {
        let state = lock(&self.shared.state);
        let item = state
            .items
            .iter()
            .find(|item| item.address == address)
            .ok_or_else(|| format!("no tray icon at {address}"))?;
        item.read.as_ref().map(pick).ok_or_else(|| format!("{address} isn't read yet"))
    }

    /// The first item published (one whose id and title were read) that
    /// `wanted` picks.
    fn item(&self, wanted: impl Fn(&TrayItem) -> bool) -> Option<TrayItem> {
        let state = lock(&self.shared.state);
        self.shared.event(&state).items.into_iter().find(|item| wanted(item))
    }

    /// Calls `Activate` on the item at `address`. An item that turns out to
    /// have none (its app didn't say what it has) is menu only from then on.
    fn send_activate(&self, address: &str) -> Result<Sent, String> {
        let Some(connection) = lock(&self.shared.connection).clone() else {
            return Ok(Sent::NotOnBus);
        };
        let slash = address.find('/').ok_or_else(|| format!("bad tray address {address}"))?;
        let (bus, path) = address.split_at(slash);
        match zbus::block_on(connection.call_method(Some(bus), path, Some(ITEM_INTERFACE), "Activate", &(0i32, 0i32))) {
            Ok(_) => Ok(Sent::Activated),
            Err(e) if error_name(&e).as_deref() == Some(UNKNOWN_METHOD) => {
                self.shared.logger.info(LOG_SOURCE, &format!("{address} has no Activate, so it shows its menu"));
                self.shared.mark_menu_only(address);
                Ok(Sent::MenuOnly)
            }
            Err(e) => Err(e.to_string()),
        }
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
                        app: self.apps.find_app(&read.app_ids),
                        menu_only: read.menu_only,
                        icon: read.look.icon.clone(),
                        attention: read.look.attention,
                        has_menu: read.look.menu_path.is_some(),
                        is_menu: read.look.is_menu,
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
        let serial = state.next_serial;
        state.next_serial += 1;
        state.items.push(Item { address: address.clone(), bus: bus.to_string(), owner, serial, read: None });
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
        let menu_only = !can_activate(connection, &owner, path).await;
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
        let props = item_props(connection, &owner, path).await;
        let app_ids = app::candidates(&self.proc_root, pid.as_ref().ok().copied(), &id);
        let bridged = pid.as_ref().is_ok_and(|&pid| app::is_bridge(&self.proc_root, pid));
        let mut state = lock(&self.state);
        let Some(item) = state.items.iter_mut().find(|item| item.address == address && item.owner == owner) else {
            return;
        };
        self.logger.info(LOG_SOURCE, &format!("{address} came: {id} ({})", app_ids.join(", ")));
        let look = self.pictures().look(&props, item.serial, None);
        item.read = Some(Read { id, title, app_ids, bridged, menu_only, look, menu_revision: None });
        self.publish(&mut state);
    }

    fn pictures(&self) -> look::Pictures<'_> {
        look::Pictures { icons: &self.icons, folder: &self.pictures }
    }

    /// The item at `path` on `sender` said its picture, status or menu
    /// changed: reads them again.
    async fn look_changed(&self, connection: &Connection, sender: &str, path: &str) {
        let known = lock(&self.state)
            .items
            .iter()
            .any(|item| item.owner == sender && item.path() == path && item.read.is_some());
        if !known {
            return;
        }
        let props = item_props(connection, sender, path).await;
        let mut state = lock(&self.state);
        let Some(item) = state.items.iter_mut().find(|item| item.owner == sender && item.path() == path) else {
            return;
        };
        let serial = item.serial;
        let Some(read) = item.read.as_mut() else { return };
        let look = self.pictures().look(&props, serial, read.look.written.as_deref());
        if look.menu_path != read.look.menu_path {
            read.menu_revision = None;
        }
        read.look = look;
        self.publish(&mut state);
    }

    /// The menu at `path` on `sender` is at `revision` now, or was read there.
    fn saw_revision(&self, sender: &str, path: &str, revision: u32) {
        let mut state = lock(&self.state);
        for item in state.items.iter_mut().filter(|item| item.owner == sender) {
            if let Some(read) = item.read.as_mut().filter(|read| read.look.menu_path.as_deref() == Some(path)) {
                read.menu_revision = Some(read.menu_revision.map_or(revision, |seen| seen.max(revision)));
            }
        }
    }

    /// Marks the item at `address` as having no `Activate`.
    fn mark_menu_only(&self, address: &str) {
        let mut state = lock(&self.state);
        let read = state.items.iter_mut().find(|item| item.address == address).and_then(|item| item.read.as_mut());
        if let Some(read) = read {
            read.menu_only = true;
            self.publish(&mut state);
        }
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
        for written in gone.iter().filter_map(|item| item.read.as_ref()?.look.written.as_deref()) {
            look::remove(written);
        }
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
