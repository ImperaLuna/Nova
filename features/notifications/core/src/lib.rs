mod bus;
mod image;
mod sent;
#[cfg(test)]
mod tests;
mod text;

use futures_core::Stream;
use nova_apps::AppIndex;
use nova_base::{lock, state_file};
use nova_carrier::log::LogLevel;
use nova_hub::{ClientId, Hub};
use nova_icons::Icons;
use nova_log::Logger;
use nova_notifications_api::{
    Event, MAX_NOTIFICATIONS, Notification, NotificationAction, NotificationApp, NotificationDevice, NotificationList,
    Own, Post, Urgency,
};
use nova_session_bus::{self as session_bus, Bus, Stop};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};
use zbus::fdo::RequestNameReply;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::wire::{OwnedVariant, Variant};
use zbus::{Connection, MatchRule, MessageStream, interface};

pub const FEATURE_ID: &str = "notifications";
const LOG_SOURCE: &str = "notifications";

pub const BUS_NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

const MAX_SUMMARY_CHARS: usize = 256;
const MAX_BODY_CHARS: usize = 2048;
const MAX_LABEL_CHARS: usize = 64;
const MAX_ACTIONS: usize = 8;
const MAX_APPS: usize = 64;
const MAX_MUTED: usize = MAX_APPS;
const MAX_TRANSIENT: usize = 8;
const MAX_NAME_LEN: usize = 255;
const KEEP_SECS: i64 = 7 * 24 * 60 * 60;
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const SAVE_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
enum Closed {
    Dismissed = 2,
    ByApp = 3,
    Other = 4,
}

#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    pub state_file: PathBuf,
    pub images: PathBuf,
    pub icons: Arc<Icons>,
}

impl Sources {
    pub fn new(state_dir: &Path, icons: Arc<Icons>) -> Self {
        Self {
            bus: Bus::Session,
            state_file: state_dir.join("notifications.json"),
            images: state_dir.join("notifications"),
            icons,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Clicked {
    pub app_id: Option<String>,
    pub heard: bool,
}

pub struct Notifications {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    sources: Sources,
    apps: Arc<AppIndex>,
    connection: Mutex<Option<Connection>>,
    revision: AtomicU64,
    stop: Stop,
}

#[derive(Default)]
struct State {
    saved: Saved,
    transient: VecDeque<Notification>,
    resident: HashSet<u32>,
    senders: HashMap<u32, String>,
    sent: Option<NotificationList>,
    written: Option<Saved>,
    written_at: Option<Instant>,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Saved {
    #[serde(default)]
    next_id: u32,
    #[serde(default)]
    do_not_disturb: bool,
    #[serde(default)]
    muted: Vec<String>,
    #[serde(default)]
    apps: Vec<NotificationApp>,
    #[serde(default)]
    items: Vec<Notification>,
}

pub struct Sent {
    pub app_name: String,
    pub replaces_id: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub hints: HashMap<String, OwnedVariant>,
    pub expire_timeout: i32,
}

impl Notifications {
    pub fn start(sources: Sources, apps: Arc<AppIndex>, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let saved = load(&sources, &logger);
        let shared = Arc::new(Shared {
            state: Mutex::new(State { written: Some(saved.clone()), saved, ..State::default() }),
            changed: Condvar::new(),
            hub,
            logger,
            sources,
            apps,
            connection: Mutex::new(None),
            revision: AtomicU64::new(jiff::Timestamp::now().as_millisecond().unsigned_abs()),
            stop: Stop::default(),
        });
        let saver = Arc::clone(&shared);
        std::thread::spawn(move || saver.save_later());
        let (ready, started) = std::sync::mpsc::channel();
        let serving = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(bus::serve(&serving, &ready)) {
                serving.logger.warn(LOG_SOURCE, &format!("notifications won't show: {e}"));
            }
            let _ = ready.send(());
        });
        let _ = started.recv_timeout(2 * CALL_TIMEOUT);
        Arc::new(Self { shared })
    }

    pub fn send_own(&self, own: Own) -> u32 {
        let mut hints = HashMap::new();
        if own.critical
            && let Ok(critical) = OwnedVariant::try_from(Variant::U8(2))
        {
            hints.insert("urgency".to_string(), critical);
        }
        let sent = Sent {
            app_name: own.app,
            replaces_id: own.replaces,
            app_icon: String::new(),
            summary: own.summary,
            body: own.body,
            actions: Vec::new(),
            hints,
            expire_timeout: -1,
        };
        let (id, gone) = self.shared.notify(sent, None, own.device);
        for old in gone {
            self.shared.signal("NotificationClosed", &(old, Closed::Other as u32));
        }
        id
    }

    pub fn listed(&self, matches: impl Fn(&Notification) -> bool) -> Option<u32> {
        lock(&self.shared.state).saved.items.iter().find(|kept| matches(kept)).map(|kept| kept.id)
    }

    pub fn stop(&self) {
        let mut state = lock(&self.shared.state);
        self.shared.save(&mut state);
        self.shared.stop.stop();
        self.shared.changed.notify_all();
    }

    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Notifications(list(&state)));
    }

    pub fn click(&self, id: u32) -> Result<Clicked, String> {
        let app_id = find(&lock(&self.shared.state), id).ok_or_else(|| format!("no notification {id}"))?.app_id.clone();
        let heard = match self.action(id, None) {
            Ok(()) => true,
            Err(e) if find(&lock(&self.shared.state), id).is_none() => return Err(e),
            Err(_) => false,
        };
        if !heard {
            self.read(Some(id));
        }
        Ok(Clicked { app_id, heard })
    }

    pub fn action(&self, id: u32, action: Option<&str>) -> Result<(), String> {
        let (key, closes) = {
            let mut state = lock(&self.shared.state);
            let notification = find(&state, id).ok_or_else(|| format!("no notification {id}"))?;
            if !notification.live {
                return Err(format!("the app that sent notification {id} has gone"));
            }
            let key = match action {
                Some(key) if notification.actions.iter().any(|known| known.key == key) => key.to_string(),
                Some(key) => return Err(format!("notification {id} has no button {key:?}")),
                None if notification.has_default => "default".to_string(),
                None => return Err(format!("notification {id} does nothing when clicked")),
            };
            let closes = action.is_none() || !state.resident.contains(&id);
            if closes {
                self.shared.retire(&mut state, id, true);
            } else if let Some(item) = state.saved.items.iter_mut().find(|item| item.id == id && item.unread) {
                item.unread = false;
                self.shared.save_and_publish(&mut state);
            }
            (key, closes)
        };
        self.shared.signal("ActionInvoked", &(id, key.as_str()));
        if closes {
            self.shared.signal("NotificationClosed", &(id, Closed::Dismissed as u32));
        }
        Ok(())
    }

    pub fn dismiss(&self, id: u32) {
        self.close(|kept| kept.id == id, Closed::Dismissed);
    }

    pub fn clear(&self, app: Option<&str>) {
        let gone = {
            let mut state = lock(&self.shared.state);
            let muted = state.saved.muted.clone();
            self.shared.remove(&mut state, |kept| app.is_none_or(|app| kept.app == app) && !muted.contains(&kept.app))
        };
        for id in gone {
            self.shared.signal("NotificationClosed", &(id, Closed::Dismissed as u32));
        }
    }

    pub fn read(&self, id: Option<u32>) {
        let mut state = lock(&self.shared.state);
        let mut changed = false;
        for item in state.saved.items.iter_mut().filter(|item| item.unread && id.is_none_or(|id| item.id == id)) {
            item.unread = false;
            changed = true;
        }
        if changed {
            self.shared.save_and_publish(&mut state);
        }
    }

    pub fn set_do_not_disturb(&self, on: bool) {
        let mut state = lock(&self.shared.state);
        if state.saved.do_not_disturb != on {
            state.saved.do_not_disturb = on;
            self.shared.logger.info(LOG_SOURCE, &format!("do not disturb {}", if on { "on" } else { "off" }));
            self.shared.save_and_publish(&mut state);
        }
    }

    pub fn mute(&self, app: &str, muted: bool) -> Result<(), String> {
        let mut state = lock(&self.shared.state);
        let at = state.saved.muted.iter().position(|kept| kept == app);
        match (muted, at) {
            (true, None) => {
                if !state.saved.apps.iter().any(|known| known.name == app) {
                    return Err(format!("no app {app:?} has sent a notification"));
                }
                if state.saved.muted.len() >= MAX_MUTED {
                    return Err(format!("at most {MAX_MUTED} apps can be muted"));
                }
                state.saved.muted.push(app.to_string());
                for item in state.saved.items.iter_mut().filter(|item| item.app == app) {
                    item.unread = false;
                }
            }
            (false, Some(at)) => {
                state.saved.muted.remove(at);
            }
            (true, Some(_)) | (false, None) => return Ok(()),
        }
        self.shared.save_and_publish(&mut state);
        Ok(())
    }

    fn close(&self, matches: impl Fn(&Notification) -> bool, why: Closed) {
        let gone = self.shared.remove(&mut lock(&self.shared.state), matches);
        for id in gone {
            self.shared.signal("NotificationClosed", &(id, why as u32));
        }
    }
}

impl Post for Notifications {
    fn post(&self, own: Own) -> u32 {
        self.send_own(own)
    }

    fn listed(&self, matches: &dyn Fn(&Notification) -> bool) -> Option<u32> {
        Notifications::listed(self, matches)
    }
}

impl Shared {
    fn notify(&self, sent: Sent, sender: Option<String>, device: Option<NotificationDevice>) -> (u32, Vec<u32>) {
        let now = jiff::Timestamp::now().as_second();
        let mut sent = sent;
        let transient = sent::take_bool(&mut sent.hints, "transient");
        let resident = sent::take_bool(&mut sent.hints, "resident");
        let mut state = lock(&self.state);
        if self.stop.stopped() {
            return (0, Vec::new());
        }
        let id = match sent.replaces_id {
            0 => None,
            id => find(&state, id).map(|_| id),
        }
        .unwrap_or_else(|| next_id(&mut state));
        let notification = self.read(id, sent, now);
        let notification = Notification { unread: !transient, device, ..notification };
        if state.saved.muted.contains(&notification.app) {
            for old in state.saved.items.iter().chain(&state.transient).filter(|old| old.id == id) {
                self.forget_image(old);
            }
            self.forget_image(&notification);
            state.saved.items.retain(|kept| kept.id != id);
            state.transient.retain(|kept| kept.id != id);
            state.resident.remove(&id);
            state.senders.remove(&id);
            remember_app(&mut state.saved, &notification);
            self.save_and_publish(&mut state);
            return (id, Vec::new());
        }
        for old in state.saved.items.iter().chain(&state.transient) {
            if old.id == id && old.image != notification.image {
                self.forget_image(old);
            }
        }
        state.saved.items.retain(|kept| kept.id != id);
        state.transient.retain(|kept| kept.id != id);
        if resident {
            state.resident.insert(id);
        } else {
            state.resident.remove(&id);
        }
        match sender {
            Some(sender) => state.senders.insert(id, sender),
            None => state.senders.remove(&id),
        };
        remember_app(&mut state.saved, &notification);
        let pops = !state.saved.do_not_disturb || notification.urgency == Urgency::Critical;
        let mut gone = Vec::new();
        if transient {
            state.transient.push_back(notification.clone());
            while state.transient.len() > MAX_TRANSIENT
                && let Some(old) = state.transient.pop_front()
            {
                gone.extend(self.forget(&mut state, &old));
            }
        } else {
            state.saved.items.insert(0, notification.clone());
            let mut kept = 0;
            for old in std::mem::take(&mut state.saved.items) {
                if kept < MAX_NOTIFICATIONS && old.time >= now - KEEP_SECS {
                    kept += 1;
                    state.saved.items.push(old);
                } else {
                    gone.extend(self.forget(&mut state, &old));
                }
            }
        }
        self.save_and_publish(&mut state);
        if pops {
            self.hub.publish(&Event::NotificationPopup(notification));
        }
        (id, gone)
    }

    fn remove(&self, state: &mut State, matches: impl Fn(&Notification) -> bool) -> Vec<u32> {
        let mut gone = Vec::new();
        let (removed, kept) = std::mem::take(&mut state.saved.items).into_iter().partition::<Vec<_>, _>(&matches);
        state.saved.items = kept;
        let (removed_transient, kept) =
            std::mem::take(&mut state.transient).into_iter().partition::<Vec<_>, _>(&matches);
        state.transient = kept.into();
        for notification in removed.iter().chain(&removed_transient) {
            gone.extend(self.forget(state, notification));
        }
        if !removed.is_empty() || !removed_transient.is_empty() {
            self.save_and_publish(state);
        }
        gone
    }

    fn retire(&self, state: &mut State, id: u32, read: bool) {
        if state.transient.iter().any(|kept| kept.id == id) {
            self.remove(state, |kept| kept.id == id);
            return;
        }
        if let Some(item) = state.saved.items.iter_mut().find(|item| item.id == id) {
            item.unread &= !read;
            item.live = false;
        }
        state.resident.remove(&id);
        state.senders.remove(&id);
        self.save_and_publish(state);
    }

    fn forget(&self, state: &mut State, notification: &Notification) -> Option<u32> {
        self.forget_image(notification);
        state.resident.remove(&notification.id);
        state.senders.remove(&notification.id);
        notification.live.then_some(notification.id)
    }

    fn forget_image(&self, notification: &Notification) {
        if let Some(image) = notification.image.as_deref().map(Path::new)
            && image::is_saved(&self.sources.images, notification.id, image)
        {
            let _ = std::fs::remove_file(image);
        }
    }

    fn sender_gone(&self, name: &str) {
        let mut state = lock(&self.state);
        let ids: HashSet<u32> =
            state.senders.iter().filter(|(_, sender)| sender.as_str() == name).map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return;
        }
        state.senders.retain(|id, _| !ids.contains(id));
        let State { saved, transient, .. } = &mut *state;
        for item in saved.items.iter_mut().chain(transient.iter_mut()) {
            if ids.contains(&item.id) {
                item.live = false;
            }
        }
        self.save_and_publish(&mut state);
    }

    fn save_and_publish(&self, state: &mut State) {
        if self.stop.stopped() {
            return;
        }
        self.changed.notify_all();
        let event = list(state);
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Notifications(event.clone()));
            state.sent = Some(event);
        }
    }

    fn save_later(&self) {
        let mut state = lock(&self.state);
        while !self.stop.stopped() {
            if state.written.as_ref() == Some(&state.saved) {
                state = self.changed.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
                continue;
            }
            let wait = state.written_at.map_or(Duration::ZERO, |at| SAVE_EVERY.saturating_sub(at.elapsed()));
            if wait.is_zero() {
                self.save(&mut state);
            } else {
                state = self.changed.wait_timeout(state, wait).unwrap_or_else(std::sync::PoisonError::into_inner).0;
            }
        }
    }

    fn save(&self, state: &mut State) {
        if state.written.as_ref() == Some(&state.saved) {
            return;
        }
        if let Err(e) = state_file::save(&self.sources.state_file, &state.saved) {
            let message = format!("can't save {}: {e}", self.sources.state_file.display());
            self.logger.warn(LOG_SOURCE, &message);
        }
        state.written = Some(state.saved.clone());
        state.written_at = Some(Instant::now());
    }

    fn signal(&self, name: &str, body: &(u32, impl serde::Serialize + zbus::wire::Type)) {
        let Some(connection) = lock(&self.connection).clone() else { return };
        if let Err(e) = zbus::block_on(connection.emit_signal(None::<()>, PATH, BUS_NAME, name, body)) {
            self.logger.warn(LOG_SOURCE, &format!("can't send {name}: {e}"));
        }
    }
}

fn load(sources: &Sources, logger: &Logger) -> Saved {
    let mut saved: Saved = match std::fs::read_to_string(&sources.state_file) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", sources.state_file.display()));
            Saved::default()
        }),
        Err(_) => Saved::default(),
    };
    let since = jiff::Timestamp::now().as_second() - KEEP_SECS;
    saved.items.retain(|item| item.time >= since);
    saved.items.truncate(MAX_NOTIFICATIONS);
    saved.apps.truncate(MAX_APPS);
    saved.muted.truncate(MAX_MUTED);
    let file = |icon: &mut Option<String>| *icon = icon.take().and_then(|icon| sent::icon(&icon, &sources.icons));
    for app in &mut saved.apps {
        file(&mut app.icon);
    }
    for item in &mut saved.items {
        file(&mut item.icon);
        item.live = false;
        if item.image.as_ref().is_some_and(|image| !Path::new(image).exists()) {
            item.image = None;
        }
    }
    let kept: HashSet<&Path> = saved.items.iter().filter_map(|item| item.image.as_deref()).map(Path::new).collect();
    for entry in std::fs::read_dir(&sources.images).into_iter().flatten().flatten() {
        let path = entry.path();
        if !kept.contains(path.as_path()) {
            let _ = std::fs::remove_file(path);
        }
    }
    saved
}

fn list(state: &State) -> NotificationList {
    NotificationList {
        items: state.saved.items.iter().filter(|item| !state.saved.muted.contains(&item.app)).cloned().collect(),
        do_not_disturb: state.saved.do_not_disturb,
        muted: state.saved.muted.clone(),
        apps: state.saved.apps.clone(),
        transient: state.transient.iter().filter(|item| !state.saved.muted.contains(&item.app)).cloned().collect(),
    }
}

fn find(state: &State, id: u32) -> Option<&Notification> {
    state.saved.items.iter().chain(&state.transient).find(|kept| kept.id == id)
}

fn next_id(state: &mut State) -> u32 {
    loop {
        state.saved.next_id = state.saved.next_id.wrapping_add(1).max(1);
        let id = state.saved.next_id;
        if find(state, id).is_none() {
            return id;
        }
    }
}

fn remember_app(saved: &mut Saved, notification: &Notification) {
    saved.apps.retain(|known| known.name != notification.app);
    let app = NotificationApp {
        name: notification.app.clone(),
        icon: notification.icon.clone(),
        app_id: notification.app_id.clone(),
    };
    saved.apps.insert(0, app);
    if saved.apps.len() > MAX_APPS
        && let Some(at) = saved.apps.iter().rposition(|known| !saved.muted.contains(&known.name))
    {
        saved.apps.remove(at);
    }
}
