//! Notifications (feature `notifications`): novad is the session's
//! notification server, `org.freedesktop.Notifications`, so notifications
//! sent while the UI restarts aren't lost and the list survives it.
//!
//! Each notification is kept for the quick settings list (at most
//! [`MAX_NOTIFICATIONS`], for a week, saved across restarts) and published as an
//! [`Event::Notifications`]; one that should pop up is also published as an
//! [`Event::NotificationPopup`]. Clicking one keeps it, read, and so does
//! its app closing it; only dismissing or clearing takes it off. Apps can be muted (their
//! notifications hidden until they're unmuted, new ones not kept) and do
//! not disturb holds back every pop-up but critical ones. Transient
//! notifications only pop up and aren't listed.
//!
//! The list is saved to a file at most once every [`SAVE_EVERY`], on a
//! thread of its own, so an app updating a notification many times a second
//! (a download's progress) doesn't rewrite it each time.
//!
//! Anything on the session bus can send one, so what apps send is checked
//! and cut down: text is made plain (`text`), pictures are checked and
//! written out as PNGs (`image`), and lists are capped. App icons given by
//! name are looked up as files (`crate::icons`), so the UI only loads images.

pub(crate) mod image;
#[cfg(test)]
mod tests;
mod text;

use crate::apps::AppIndex;
use crate::hub::{ClientId, Hub};
use crate::icons::Icons;
use crate::logger::Logger;
use crate::session_bus::{self, Bus, Stop};
use crate::{lock, state_file};
use futures_core::Stream;
use nova_protocol::{
    Event, LogLevel, MAX_NOTIFICATIONS, Notification, NotificationAction, NotificationApp, NotificationDevice,
    NotificationList, Urgency,
};
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

/// Summaries are cut to this many characters, app names and button labels
/// to [`MAX_LABEL_CHARS`], bodies to this.
const MAX_SUMMARY_CHARS: usize = 256;
const MAX_BODY_CHARS: usize = 2048;
const MAX_LABEL_CHARS: usize = 64;
/// More buttons than a notification has room for; the rest are dropped.
const MAX_ACTIONS: usize = 8;
/// Apps on the settings page; past it, the oldest unmuted one goes.
const MAX_APPS: usize = 64;
/// How many apps can be muted.
const MAX_MUTED: usize = MAX_APPS;
/// Transient notifications kept for their pop-ups' clicks.
const MAX_TRANSIENT: usize = 8;
/// Desktop entry ids and icon names are at most this long.
const MAX_NAME_LEN: usize = 255;
/// Notifications older than this (a week, in seconds) leave the list.
const KEEP_SECS: i64 = 7 * 24 * 60 * 60;
/// How long novad waits for the bus to answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The state file is written at most this often. A change in the last one
/// is lost when novad is killed before it's written.
const SAVE_EVERY: Duration = Duration::from_secs(1);

/// Why a notification closed, as `NotificationClosed` tells its app.
#[derive(Clone, Copy)]
enum Closed {
    Dismissed = 2,
    /// The app closed it (`CloseNotification`).
    ByApp = 3,
    /// Pushed off the end of the list.
    Other = 4,
}

/// Where the server is, and what it keeps.
#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    /// The list, do not disturb and the muted apps.
    pub state_file: PathBuf,
    /// Pictures apps sent as pixels, as PNGs.
    pub images: PathBuf,
    /// Where apps' icons are found.
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

/// A notification of Nova's own (a device's low battery), sent with
/// [`Notifications::send_own`].
#[derive(Debug, Clone, PartialEq)]
pub struct Own {
    /// Who it reads as from, on its app line.
    pub app: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    /// The id of an earlier one it takes the place of, or 0.
    pub replaces: u32,
    pub device: Option<NotificationDevice>,
}

/// What a click on a notification ([`Notifications::click`]) did.
#[derive(Debug, PartialEq)]
pub struct Clicked {
    /// The app that sent it, by its desktop file id, when it said.
    pub app_id: Option<String>,
    /// Its app heard of the click (`ActionInvoked`): it's still there and
    /// does something when the notification is clicked.
    pub heard: bool,
}

pub struct Notifications {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    /// Wakes the saver (`save_later`) when the list changed or novad stops.
    changed: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    sources: Sources,
    /// The installed apps, whose names and icons a notification naming its
    /// desktop entry shows.
    apps: Arc<AppIndex>,
    /// The server's connection, once it's up, to tell apps about clicks.
    connection: Mutex<Option<Connection>>,
    /// The next picture's revision (`image::save`).
    revision: AtomicU64,
    stop: Stop,
}

#[derive(Default)]
struct State {
    saved: Saved,
    /// Notifications that pop up but aren't listed, the newest last.
    transient: VecDeque<Notification>,
    /// Ids of notifications that stay when one of their buttons is clicked.
    resident: HashSet<u32>,
    /// Which connection on the bus sent each live notification; when it
    /// goes, its notifications can't be clicked through to it any more.
    senders: HashMap<u32, String>,
    /// The last list published, so an unchanged one isn't sent again.
    sent: Option<NotificationList>,
    /// What the state file holds, so an unchanged list isn't written again,
    /// and when it was written.
    written: Option<Saved>,
    written_at: Option<Instant>,
}

/// What survives a restart.
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

/// A notification as an app sends it with `Notify`.
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
    /// Claims the server's name on the bus and serves it on a thread of its
    /// own. Returns once the name is claimed, queued behind another server,
    /// or connecting failed (logged).
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
            // From the clock, so a revision is never one an earlier run used.
            revision: AtomicU64::new(jiff::Timestamp::now().as_millisecond().unsigned_abs()),
            stop: Stop::default(),
        });
        let saver = Arc::clone(&shared);
        std::thread::spawn(move || saver.save_later());
        let (ready, started) = std::sync::mpsc::channel();
        let serving = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(serve(&serving, &ready)) {
                serving.logger.warn(LOG_SOURCE, &format!("notifications won't show: {e}"));
            }
            let _ = ready.send(());
        });
        // Waits as long as connecting may take; a timed-out call fails sooner.
        let _ = started.recv_timeout(2 * CALL_TIMEOUT);
        Arc::new(Self { shared })
    }

    /// Shows a notification of Nova's own, as if an app sent it, and
    /// returns its id (0 once stopped).
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

    /// The newest listed notification `matches` picks, by id: the one a
    /// notification of Nova's own takes the place of when it knows no
    /// earlier id, such as after a restart.
    pub fn listed(&self, matches: impl Fn(&Notification) -> bool) -> Option<u32> {
        lock(&self.shared.state).saved.items.iter().find(|kept| matches(kept)).map(|kept| kept.id)
    }

    /// Writes what's still unsaved, lets go of the server's name and
    /// publishes nothing more.
    pub fn stop(&self) {
        let mut state = lock(&self.shared.state);
        self.shared.save(&mut state);
        self.shared.stop.stop();
        self.shared.changed.notify_all();
    }

    /// Sends a new subscriber, client `id`, the list as it is, under the
    /// lock that publishing takes, so a newer list can't arrive before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Notifications(list(&state)));
    }

    /// The notification `id` itself was clicked: its app hears of it when
    /// it can, and it's read either way. What novad does next, the click
    /// taken or missed, goes by what's returned.
    pub fn click(&self, id: u32) -> Result<Clicked, String> {
        let app_id = find(&lock(&self.shared.state), id).ok_or_else(|| format!("no notification {id}"))?.app_id.clone();
        let heard = match self.action(id, None) {
            Ok(()) => true,
            // Taken off the list meanwhile (dismissed, cleared): nothing to open.
            Err(e) if find(&lock(&self.shared.state), id).is_none() => return Err(e),
            Err(_) => false,
        };
        if !heard {
            self.read(Some(id));
        }
        Ok(Clicked { app_id, heard })
    }

    /// The notification `id` was clicked (`action` `None`) or one of its
    /// buttons: its app hears of it, and it closes unless the app keeps it.
    /// Either way it's read; closed, it stays on the list as one its app no
    /// longer hears about (clicking it again opens the app). An error when its app can't
    /// hear of it.
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
            // A click on the notification itself always closes it.
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

    /// Closes the notification `id` and takes it off the list.
    pub fn dismiss(&self, id: u32) {
        self.close(|kept| kept.id == id, Closed::Dismissed);
    }

    /// Takes every notification off the list, or only `app`'s; a muted
    /// app's hidden ones stay for when it's unmuted.
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

    /// Marks every notification read, or only `id`.
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

    /// Mutes `app`, hiding its notifications, or unmutes it, showing them
    /// again. Only an app on the settings page can be muted.
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
                // Shown again on unmuting, they've been seen.
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

    /// Takes the notifications `matches` picks off the list (and pop-ups),
    /// telling their apps why.
    fn close(&self, matches: impl Fn(&Notification) -> bool, why: Closed) {
        let gone = self.shared.remove(&mut lock(&self.shared.state), matches);
        for id in gone {
            self.shared.signal("NotificationClosed", &(id, why as u32));
        }
    }
}

impl Shared {
    /// Takes a notification an app, the bus connection `sender`, sent, or
    /// Nova itself about `device`. Returns its id, and the ones it pushed
    /// off the list, whose apps are to be told.
    fn notify(&self, sent: Sent, sender: Option<String>, device: Option<NotificationDevice>) -> (u32, Vec<u32>) {
        let now = jiff::Timestamp::now().as_second();
        let mut sent = sent;
        let transient = take_bool(&mut sent.hints, "transient");
        let resident = take_bool(&mut sent.hints, "resident");
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
        // A muted app's notification isn't kept, nor the one it replaces.
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
        // A picture the one it replaces had, and it doesn't, goes.
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

    /// What `sent` becomes as notification `id`, its text and pictures checked.
    fn read(&self, id: u32, mut sent: Sent, now: i64) -> Notification {
        let desktop_entry = take_str(&mut sent.hints, "desktop-entry")
            .map(|entry| entry.strip_suffix(".desktop").unwrap_or(&entry).to_string())
            .filter(|entry| is_name(entry));
        // Like GNOME: the installed app the sender names is shown as its
        // desktop entry says (the user may have changed its name or icon
        // there), and what the sender calls itself only when none is. Right
        // after novad starts, it waits for the apps to be read, or a muted
        // app would get past its mute under the sender's name.
        let installed =
            desktop_entry.as_deref().and_then(|entry| self.apps.app_once_scanned(&format!("{entry}.desktop")));
        let (installed_name, installed_icon) = installed.map_or((None, None), |app| (Some(app.name), app.icon));
        let app = [installed_name.as_deref(), Some(sent.app_name.as_str()), desktop_entry.as_deref()]
            .into_iter()
            .flatten()
            .map(|name| text::cut(name.trim(), MAX_LABEL_CHARS))
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown app".into());
        let icons = &self.sources.icons;
        let icon = installed_icon
            .or_else(|| icon(&sent.app_icon, icons))
            .or_else(|| desktop_entry.as_deref().and_then(|entry| icons.find(entry)));
        let urgency = match take(&mut sent.hints, "urgency").as_deref() {
            Some(Variant::U8(0)) => Urgency::Low,
            Some(Variant::U8(2)) => Urgency::Critical,
            _ => Urgency::Normal,
        };
        let progress = match take(&mut sent.hints, "value").as_deref() {
            Some(Variant::I32(value)) => Some((*value).clamp(0, 100) as u8),
            Some(Variant::U32(value)) => Some((*value).min(100) as u8),
            _ => None,
        };
        let image = self.image(id, &mut sent.hints);
        let mut actions = Vec::new();
        let mut has_default = false;
        for [key, label] in sent.actions.as_chunks::<2>().0 {
            let label = text::cut(label.trim(), MAX_LABEL_CHARS);
            if key == "default" {
                has_default = true;
            } else if !key.is_empty()
                && key.len() <= MAX_NAME_LEN
                && !label.is_empty()
                && actions.len() < MAX_ACTIONS
                && !actions.iter().any(|known: &NotificationAction| &known.key == key)
            {
                actions.push(NotificationAction { key: key.clone(), label });
            }
        }
        // Critical ones stay until they're closed, whatever the app asked.
        let timeout_ms = match (urgency, sent.expire_timeout) {
            (Urgency::Critical, _) => Some(0),
            (_, timeout) => u32::try_from(timeout).ok(),
        };
        Notification {
            id,
            app,
            icon,
            app_id: desktop_entry.map(|entry| format!("{entry}.desktop")),
            summary: text::cut(sent.summary.replace(['\n', '\r'], " ").trim(), MAX_SUMMARY_CHARS),
            body: text::plain(&sent.body, MAX_BODY_CHARS),
            image,
            actions,
            has_default,
            urgency,
            progress,
            timeout_ms,
            time: now,
            unread: false,
            live: true,
            device: None,
        }
    }

    /// The picture in `hints`: pixels, written out, or a file.
    fn image(&self, id: u32, hints: &mut HashMap<String, OwnedVariant>) -> Option<String> {
        // Older senders use the spec's earlier names.
        for key in ["image-data", "image_data", "icon_data"] {
            let Some(raw) = hints.remove(key) else { continue };
            let saved = image::Raw::try_from(raw).map_err(|e| format!("not a picture: {e}")).and_then(|raw| {
                image::save(raw, &self.sources.images, id, self.revision.fetch_add(1, Ordering::Relaxed))
            });
            match saved {
                Ok(path) => return Some(path.display().to_string()),
                Err(e) => self.logger.log(LogLevel::Debug, LOG_SOURCE, &format!("skipped {key}: {e}")),
            }
        }
        ["image-path", "image_path"]
            .into_iter()
            .find_map(|key| take_str(hints, key))
            .and_then(|reference| image::file(&reference))
            .map(|path| path.display().to_string())
    }

    /// Takes the notifications `matches` picks off the list and pop-ups,
    /// and says which of them have an app to tell.
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

    /// Notification `id` closed but stays listed, no longer its app's to
    /// hear about, and `read` if so. A transient one, not listed, goes.
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

    /// What novad keeps about `notification`, taken off the list: its
    /// picture and who sent it. Its id, when its app is to be told.
    fn forget(&self, state: &mut State, notification: &Notification) -> Option<u32> {
        self.forget_image(notification);
        state.resident.remove(&notification.id);
        state.senders.remove(&notification.id);
        notification.live.then_some(notification.id)
    }

    /// Deletes the picture novad wrote for `notification`, if it wrote one.
    fn forget_image(&self, notification: &Notification) {
        if let Some(image) = notification.image.as_deref().map(Path::new)
            && image::is_saved(&self.sources.images, notification.id, image)
        {
            let _ = std::fs::remove_file(image);
        }
    }

    /// The bus connection `name` went: the notifications it sent stay
    /// listed, but clicking one opens its app instead.
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

    /// Publishes the list and has the saver write it, under the state lock
    /// so changes reach the UI in the order they were made.
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

    /// The saver's thread: writes the list once it changed, at most once
    /// every [`SAVE_EVERY`], until novad stops.
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

    /// Writes the list to the state file, unless it holds it already.
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

    /// Sends the signal `name` to every app on the bus.
    fn signal(&self, name: &str, body: &(u32, impl serde::Serialize + zbus::wire::Type)) {
        let Some(connection) = lock(&self.connection).clone() else { return };
        if let Err(e) = zbus::block_on(connection.emit_signal(None::<()>, PATH, BUS_NAME, name, body)) {
            self.logger.warn(LOG_SOURCE, &format!("can't send {name}: {e}"));
        }
    }
}

/// The saved list, or an empty one. Kept notifications' apps are gone, as
/// are pictures that went with novad's last run; pictures of ones not kept
/// (too old, or transient) are deleted.
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
    // Kept before icons were files, by name.
    let file = |icon: &mut Option<String>| *icon = icon.take().and_then(|icon| self::icon(&icon, &sources.icons));
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

/// The next id no notification has; never 0, which means none.
fn next_id(state: &mut State) -> u32 {
    loop {
        state.saved.next_id = state.saved.next_id.wrapping_add(1).max(1);
        let id = state.saved.next_id;
        if find(state, id).is_none() {
            return id;
        }
    }
}

/// Puts `notification`'s app first on the settings page.
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

/// The file for an app icon as `Notify` gives it: a file, or a theme icon
/// name looked up in `icons`.
fn icon(reference: &str, icons: &Icons) -> Option<String> {
    let reference = reference.trim();
    if let Some(path) = image::file(reference) {
        return Some(path.display().to_string());
    }
    is_name(reference).then(|| icons.find(reference)).flatten()
}

/// A desktop entry id or icon name: short, and nothing that reads as a path.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
        && !name.starts_with('.')
}

fn take(hints: &mut HashMap<String, OwnedVariant>, key: &str) -> Option<OwnedVariant> {
    hints.remove(key)
}

fn take_bool(hints: &mut HashMap<String, OwnedVariant>, key: &str) -> bool {
    matches!(take(hints, key).as_deref(), Some(Variant::Bool(true)))
}

fn take_str(hints: &mut HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
    match take(hints, key).as_deref() {
        Some(Variant::Str(text)) => Some(text.as_str().to_string()),
        _ => None,
    }
}

/// Connects to the bus, serves the server and claims its name, until `stop`
/// or the bus goes. Sends on `ready` once apps can send notifications.
async fn serve(shared: &Arc<Shared>, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let server = Server { shared: Arc::clone(shared) };
    let connection = shared.sources.bus.builder().method_timeout(CALL_TIMEOUT).serve_at(PATH, server).build().await?;
    // Queued behind another server (mako, dunst, another shell's), novad
    // takes over when it quits.
    match session_bus::claim(&connection, BUS_NAME).await? {
        RequestNameReply::InQueue => shared.logger.warn(
            LOG_SOURCE,
            &format!("another program owns {BUS_NAME}; its notifications show instead until it quits"),
        ),
        _ => shared.logger.info(LOG_SOURCE, &format!("showing notifications as {BUS_NAME}")),
    }
    // Apps that quit leave the bus; their notifications can't hear clicks.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .member("NameOwnerChanged")
        .build()?;
    let mut owners = MessageStream::for_match_rule(rule, &connection, None).await?;
    *lock(&shared.connection) = Some(connection.clone());
    let _ = ready.send(());
    let result = loop {
        let next = poll_fn(|cx| match shared.stop.poll(cx) {
            Poll::Ready(()) => Poll::Ready(None),
            Poll::Pending => Pin::new(&mut owners).poll_next(cx).map(Some),
        });
        // Stopped, or the bus went.
        let Some(Some(message)) = next.await else { break Ok(()) };
        let changed = message.and_then(|message| message.body().deserialize::<(String, String, String)>());
        match changed {
            Ok((name, _, owner)) if owner.is_empty() => shared.sender_gone(&name),
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    *lock(&shared.connection) = None;
    result
}

/// The server apps see on the bus.
struct Server {
    shared: Arc<Shared>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[expect(clippy::too_many_arguments, reason = "the spec's signature")]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedVariant>,
        expire_timeout: i32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<u32> {
        let sent = Sent { app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout };
        let (id, gone) = self.shared.notify(sent, header.sender().map(|sender| sender.to_string()), None);
        for old in gone {
            Self::notification_closed(&emitter, old, Closed::Other as u32).await?;
        }
        Ok(id)
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        // It stays listed, and its pop-up runs its time: chat apps close
        // the last message's notification as they send the next one (a
        // web notification's tag), which would otherwise take it away.
        let closed = {
            let mut state = lock(&self.shared.state);
            let live = find(&state, id).is_some_and(|kept| kept.live);
            if live {
                self.shared.retire(&mut state, id, false);
            }
            live
        };
        if closed {
            Self::notification_closed(&emitter, id, Closed::ByApp as u32).await?;
        }
        Ok(())
    }

    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "body-markup", "icon-static", "persistence"]
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("Nova", "Nova", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
}

/// Started before the UI, which settings start ahead of, so notifications
/// sent while it starts are kept. No options.
impl crate::modules::Module for Notifications {
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_protocol::Topic::Notifications);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::new(&env.args.state_dir, Arc::clone(env.icons))
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_protocol::FeatureState],
    ) -> Arc<Self> {
        Self::start(sources.clone(), Arc::clone(&ctx.apps), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))
    }

    fn send_state(&self, client: ClientId) {
        Self::send_state(self, client);
    }

    fn stop(&self) {
        Self::stop(self);
    }
}
