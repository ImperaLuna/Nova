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
//! what apps send it, since anything on the session bus can register.

mod app;
#[cfg(test)]
mod tests;

use crate::hub::{ClientId, Hub};
use crate::launcher::Launcher;
use crate::lock;
use crate::logger::Logger;
use futures_core::Stream;
use nova_protocol::{Event, LogLevel, Tray as TrayEvent, TrayItem};
use std::future::poll_fn;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::Duration;
use zbus::fdo::RequestNameReply;
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::wire::{ObjectPath, OwnedVariant};
use zbus::{Connection, MatchRule, MessageStream, connection, interface};

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
/// The longest service string a registration may send. Bus names are at
/// most 255 bytes; this leaves room for a path after one.
const MAX_SERVICE_LEN: usize = 512;
/// Ids and titles are cut to this many characters.
const MAX_TEXT_CHARS: usize = 256;
/// How long novad waits for an app, or the bus, to answer a call.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Which bus the watcher is on.
#[derive(Clone)]
pub enum Bus {
    /// The session's, from `DBUS_SESSION_BUS_ADDRESS`.
    Session,
    /// A bus at this address, for tests.
    #[cfg(test)]
    Address(String),
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
    /// Finds the app an item belongs to, while the `launcher` feature runs.
    launcher: Mutex<Option<Arc<Launcher>>>,
    stop: Stop,
}

#[derive(Default)]
struct State {
    items: Vec<Item>,
    /// The last event published, so an unchanged list isn't sent again.
    sent: Option<TrayEvent>,
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
}

/// Ends the bus loop: set once, and wakes the loop if it's waiting.
#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Tray {
    /// Claims the watcher's name on `bus` and serves it on a thread of its
    /// own. Returns once the name is claimed, queued behind another
    /// watcher, or connecting failed (logged), so apps started after it
    /// find the watcher.
    pub fn start(
        bus: Bus,
        proc_root: PathBuf,
        launcher: Option<Arc<Launcher>>,
        hub: Arc<Hub>,
        logger: Arc<Logger>,
    ) -> Arc<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            hub,
            logger,
            proc_root,
            launcher: Mutex::new(launcher),
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
        self.shared.stop.stopped.store(true, Ordering::SeqCst);
        if let Some(waker) = lock(&self.shared.stop.waker).take() {
            waker.wake();
        }
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

    /// Sends a new subscriber, client `id`, the items as they are, under the
    /// lock that publishing takes, so a newer list can't arrive before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Tray(self.shared.event(&state)));
    }
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.stopped.load(Ordering::SeqCst)
    }

    fn event(&self, state: &State) -> TrayEvent {
        let launcher = lock(&self.launcher).clone();
        let items = state
            .items
            .iter()
            .filter_map(|item| {
                let read = item.read.as_ref()?;
                Some(TrayItem {
                    address: item.address.clone(),
                    id: read.id.clone(),
                    title: read.title.clone(),
                    app: launcher.as_ref().and_then(|launcher| launcher.find_app(&read.app_ids)),
                })
            })
            .collect();
        TrayEvent { items }
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
    /// a bus name, or an object path on the sender's connection.
    async fn register(&self, connection: &Connection, sender: &str, service: &str) -> Result<String, String> {
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
        let address = format!("{bus}{path}");
        let mut state = lock(&self.state);
        if self.stopped() {
            return Err("the tray is stopping".into());
        }
        if state.items.iter().any(|item| item.address == address) {
            return Ok(address);
        }
        if state.items.len() >= MAX_ITEMS {
            return Err(format!("the tray already has {MAX_ITEMS} items"));
        }
        state.items.push(Item { address: address.clone(), bus: bus.to_string(), owner, read: None });
        Ok(address)
    }

    /// Reads the id and title of the item at `address`, and which process
    /// it's in, then publishes it. An item whose app has gone meanwhile is
    /// dropped; one that answers wrongly shows with what could be read.
    async fn read(&self, connection: &Connection, address: &str) {
        let Some(bus) =
            lock(&self.state).items.iter().find(|item| item.address == address).map(|item| item.bus.clone())
        else {
            return;
        };
        let path = &address[bus.len()..];
        let id = item_text(connection, &bus, path, "Id").await;
        let title = item_text(connection, &bus, path, "Title").await;
        let pid = connection
            .call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetConnectionUnixProcessID", &(bus.as_str(),))
            .await
            .and_then(|reply| reply.body().deserialize::<u32>());
        if let Err(e) = &pid {
            // A name with no owner has nothing left to show.
            if matches!(e, zbus::Error::MethodError(name, ..) if name.as_str() == "org.freedesktop.DBus.Error.NameHasNoOwner")
            {
                for address in self.drop_bus(&bus) {
                    if let Ok(emitter) = SignalEmitter::new(connection, WATCHER_PATH) {
                        let _ = emitter.status_notifier_item_unregistered(&address).await;
                    }
                }
                return;
            }
            let message = format!("can't tell which process {address} is in: {e}");
            self.logger.log(LogLevel::Debug, LOG_SOURCE, &message);
        }
        let id = id.unwrap_or_default();
        let title = title.unwrap_or_default();
        let app_ids = app::candidates(&self.proc_root, pid.ok(), &id);
        let mut state = lock(&self.state);
        let Some(item) = state.items.iter_mut().find(|item| item.address == address) else {
            return;
        };
        self.logger.info(LOG_SOURCE, &format!("{address} came: {id} ({})", app_ids.join(", ")));
        item.read = Some(Read { id, title, app_ids });
        self.publish(&mut state);
    }

    /// Drops the items on `name`, a bus name that lost its owner, and says
    /// which went.
    fn drop_bus(&self, name: &str) -> Vec<String> {
        let mut state = lock(&self.state);
        let (gone, kept) = std::mem::take(&mut state.items)
            .into_iter()
            .partition::<Vec<_>, _>(|item| item.bus == name || item.owner == name);
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

/// Connects to `bus`, claims the watcher's name and serves it, dropping
/// items whose app leaves the bus, until `stop` or the bus goes. Sends on
/// `ready` once apps can register.
async fn serve(shared: &Arc<Shared>, bus: Bus, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let builder = match bus {
        Bus::Session => connection::Builder::session(),
        #[cfg(test)]
        Bus::Address(address) => connection::Builder::address(address.as_str()),
    };
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
    match connection.request_name_with_flags(WATCHER_NAME, Default::default()).await? {
        RequestNameReply::InQueue => shared.logger.warn(
            LOG_SOURCE,
            &format!("another program owns {WATCHER_NAME}; tray icons show through it until it quits"),
        ),
        _ => shared.logger.info(LOG_SOURCE, &format!("watching for tray icons as {WATCHER_NAME}")),
    }
    let _ = ready.send(());
    let emitter = SignalEmitter::new(&connection, WATCHER_PATH)?;
    while let Some(message) = next_unless_stopped(&mut owners, &shared.stop).await {
        let (name, _old, new): (String, String, String) = message?.body().deserialize()?;
        if !new.is_empty() {
            continue;
        }
        for address in shared.drop_bus(&name) {
            emitter.status_notifier_item_unregistered(&address).await?;
        }
    }
    Ok(())
}

/// The stream's next message; `None` once the connection closes or `stop` is set.
async fn next_unless_stopped(stream: &mut MessageStream, stop: &Stop) -> Option<zbus::Result<zbus::Message>> {
    poll_fn(|cx| {
        // Stored before the check, so a stop in between still wakes us.
        *lock(&stop.waker) = Some(cx.waker().clone());
        if stop.stopped.load(Ordering::SeqCst) {
            return Poll::Ready(None);
        }
        Pin::new(&mut *stream).poll_next(cx)
    })
    .await
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
        let address = self.shared.register(connection, &sender, service).await.map_err(|e| {
            self.shared.logger.warn(LOG_SOURCE, &format!("refused a tray icon from {sender}: {e}"));
            zbus::fdo::Error::InvalidArgs(e)
        })?;
        emitter.status_notifier_item_registered(&address).await?;
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
