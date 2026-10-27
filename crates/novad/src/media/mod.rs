//! Now playing (feature `media`): the MPRIS players on the session bus, each
//! one's app, track and playback, for the pill beside the AI usage pill and
//! its panel.
//!
//! novad follows every `org.mpris.MediaPlayer2.*` name with `NameOwnerChanged`
//! and reads a player when it appears, when its `PropertiesChanged` or
//! `Seeked` signal comes, and never on a timer. The player the pill shows is
//! the one that most recently started playing, else the one that most
//! recently paused, unless the user picked one with `media_show` and no
//! other has started since (see [`Shared::shown`]). Each change is published
//! as [`Event::Media`] when it changes what the UI shows. Covers are checked
//! and cached here (see `cover`); the UI only draws them.

mod cover;
mod player;
#[cfg(test)]
mod tests;

use crate::compositor::Compositor;
use crate::hub::{ClientId, Hub};
use crate::launcher::Launcher;
use crate::lock;
use crate::logger::Logger;
use crate::session_bus::{Bus, Stop};
use futures_core::Stream;
use nova_protocol::{Event, Media, MediaAction, MediaPlayer, MediaStatus};
use player::Reading;
use std::collections::HashMap;
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::{Connection, MatchRule, MessageStream, ObjectPath, OwnedVariant};

pub const FEATURE_ID: &str = "media";
const LOG_SOURCE: &str = "media";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
/// How long novad waits for a player, or the bus, to answer a call.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// More players than any desktop shows; past it, new ones are ignored.
const MAX_PLAYERS: usize = 16;
/// Text (title, artist, album, identity) is cut to this many characters.
const MAX_TEXT_CHARS: usize = 256;
/// The largest cover kept, downloaded or local.
pub const COVER_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// How many covers the cache keeps.
pub const COVER_KEEP: usize = 4;

/// Where the service is, and where covers are cached.
#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    /// `media` under novad's runtime folder: covers are cached here.
    pub cache_dir: PathBuf,
}

impl Sources {
    pub fn new(runtime_dir: &Path) -> Self {
        Self { bus: Bus::Session, cache_dir: runtime_dir.join("media") }
    }
}

pub struct NowPlaying {
    shared: Arc<Shared>,
}

/// State shared by the service's loop, its cover downloads and the requests.
struct Shared {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    cache_dir: PathBuf,
    /// Finds the app a player belongs to, while the `launcher` feature runs.
    launcher: Mutex<Option<Arc<Launcher>>>,
    /// The service's connection, once it's up, for calls to players.
    connection: Mutex<Option<Connection>>,
    stop: Stop,
}

#[derive(Default)]
struct State {
    /// In the order they appeared.
    players: Vec<Player>,
    /// Counts playback changes and `media_show` requests, so the newest wins.
    seq: u64,
    /// The player the user picked, and the `seq` it was picked at.
    shown: Option<Shown>,
    /// The last event published, so an unchanged state isn't sent again.
    sent: Option<Media>,
}

#[derive(Clone)]
struct Shown {
    player: String,
    at: u64,
}

struct Player {
    /// The bus name, `org.mpris.MediaPlayer2.<name>`.
    id: String,
    /// The unique name that owns `id` now, to match the signals it sends.
    unique: String,
    /// What it said last time it was read.
    reading: Reading,
    /// When `reading` was taken, in milliseconds since the Unix epoch.
    read_at_ms: u64,
    /// The cover file for `reading.art_url`, once there is one.
    cover: Option<PathBuf>,
    /// The `seq` when it last started playing or paused.
    activated: u64,
}

impl NowPlaying {
    /// Follows the players on `sources`' bus on a thread of its own. Returns
    /// once connected or failed (logged), so requests find the service.
    pub fn start(sources: Sources, launcher: Option<Arc<Launcher>>, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let Sources { bus, cache_dir } = sources;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            hub,
            logger,
            cache_dir,
            launcher: Mutex::new(launcher),
            connection: Mutex::new(None),
            stop: Stop::default(),
        });
        let (ready, started) = std::sync::mpsc::channel();
        let serving = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(serve(&serving, bus, &ready)) {
                serving.logger.warn(LOG_SOURCE, &format!("now playing won't show: {e}"));
            }
            let _ = ready.send(());
        });
        let _ = started.recv_timeout(2 * CALL_TIMEOUT);
        Arc::new(Self { shared })
    }

    /// Hands the service the launcher, which may have started or stopped.
    pub fn set_launcher(&self, launcher: Option<Arc<Launcher>>) {
        *lock(&self.shared.launcher) = launcher;
        let mut state = lock(&self.shared.state);
        self.shared.publish(&mut state);
    }

    /// Ends the service's loop. Its connection closes once the loop ends.
    pub fn stop(&self) {
        self.shared.stop.stop();
    }

    /// Sends the current players to one client, as a topic subscription does.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        let media = self.shared.snapshot(&state);
        self.shared.hub.send_to(id, &Event::Media(media));
    }

    /// Sends a transport control to `player`, if it can do it.
    pub fn control(&self, player: &str, action: MediaAction) -> Result<(), String> {
        let (bus, method, allowed) = {
            let state = lock(&self.shared.state);
            let found = state.find(player)?;
            let reading = &found.reading;
            let (method, allowed) = match action {
                MediaAction::PlayPause if reading.status == MediaStatus::Playing => ("Pause", reading.can_pause),
                MediaAction::PlayPause => ("Play", reading.can_play),
                MediaAction::Next => ("Next", reading.can_next),
                MediaAction::Previous => ("Previous", reading.can_previous),
            };
            (found.id.clone(), method, allowed)
        };
        if !allowed {
            return Err(format!("{player} can't do that now"));
        }
        self.shared.call(&bus, PLAYER_INTERFACE, method, &())
    }

    /// Moves `player` to `position_us` into its track, within the length.
    pub fn seek(&self, player: &str, position_us: u64) -> Result<(), String> {
        let (bus, track, target) = {
            let state = lock(&self.shared.state);
            let found = state.find(player)?;
            if !found.reading.can_seek {
                return Err(format!("{player} can't seek"));
            }
            (found.id.clone(), found.reading.track_id.clone(), position_us.min(found.reading.length_us))
        };
        let track =
            ObjectPath::try_from(track.as_str()).map_err(|_| "the player has no track to seek in".to_string())?;
        self.shared.call(&bus, PLAYER_INTERFACE, "SetPosition", &(track, target as i64))
    }

    /// Makes `player` the one the pill shows, until another starts playing.
    pub fn show(&self, player: &str) -> Result<(), String> {
        let mut state = lock(&self.shared.state);
        state.find(player)?;
        state.seq += 1;
        state.shown = Some(Shown { player: player.to_string(), at: state.seq });
        self.shared.publish(&mut state);
        Ok(())
    }

    /// The name of the workspace where `player`'s app has a window, when
    /// there is one (the compositor's answer, as `focus_app` finds it).
    pub fn window(&self, player: &str, compositor: Option<&Compositor>) -> Result<Option<String>, String> {
        let ids = {
            let state = lock(&self.shared.state);
            app_ids(&state.find(player)?.reading)
        };
        match compositor {
            Some(compositor) => compositor.window_workspace(&ids),
            None => Ok(None),
        }
    }

    /// Focuses the window of `player`'s app where it is (`compositor`), or
    /// raises the player when no window was found. Whether either worked.
    pub fn focus(&self, player: &str, compositor: Option<&Compositor>) -> Result<bool, String> {
        let (bus, ids, can_raise) = {
            let state = lock(&self.shared.state);
            let found = state.find(player)?;
            (found.id.clone(), app_ids(&found.reading), found.reading.can_raise)
        };
        if let Some(compositor) = compositor {
            match compositor.focus_app(&ids, None) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(e) => self.shared.logger.warn(LOG_SOURCE, &format!("couldn't focus {player}'s app: {e}")),
            }
        }
        if !can_raise {
            return Ok(false);
        }
        self.shared.call(&bus, ROOT_INTERFACE, "Raise", &())?;
        Ok(true)
    }
}

impl State {
    /// The player with bus name `id`, with its reading, when it has one.
    fn find(&self, id: &str) -> Result<&Player, String> {
        self.players.iter().find(|player| player.id == id).ok_or_else(|| format!("no media player {id}"))
    }

    /// The player the pill shows: the one the user picked while no other has
    /// started playing since, else the newest one playing, else the newest paused.
    fn shown_id(&self) -> Option<&str> {
        let playing_since = |status: MediaStatus| {
            self.players
                .iter()
                .filter(|player| player.reading.status == status)
                .max_by_key(|player| player.activated)
                .map(|player| (player, player.activated))
        };
        if let Some(shown) = &self.shown {
            let beaten = self
                .players
                .iter()
                .any(|player| player.reading.status == MediaStatus::Playing && player.activated > shown.at);
            if !beaten && self.players.iter().any(|player| player.id == shown.player) {
                return Some(&shown.player);
            }
        }
        playing_since(MediaStatus::Playing)
            .or_else(|| playing_since(MediaStatus::Paused))
            .map(|(player, _)| player.id.as_str())
    }
}

impl Shared {
    /// The event for the current state, for everyone or one client.
    fn snapshot(&self, state: &State) -> Media {
        let launcher = lock(&self.launcher).clone();
        let players = state
            .players
            .iter()
            .map(|player| {
                let ids = app_ids(&player.reading);
                let app = launcher.as_ref().and_then(|launcher| launcher.find_app(&ids));
                let browser = launcher.as_ref().is_some_and(|launcher| launcher.is_browser(&ids));
                wire_player(player, app, browser)
            })
            .collect();
        Media { players, shown: state.shown_id().map(str::to_string) }
    }

    /// Publishes the state if it changed since the last publish. Called with
    /// the state locked, so the change and what follows from it stay in order.
    fn publish(&self, state: &mut State) {
        let media = self.snapshot(state);
        if state.sent.as_ref() == Some(&media) {
            return;
        }
        self.hub.publish(&Event::Media(media.clone()));
        state.sent = Some(media);
    }

    /// Calls `method` on the player `bus`, on its connection, within the timeout.
    fn call<B>(&self, bus: &str, interface: &str, method: &str, body: &B) -> Result<(), String>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let connection = lock(&self.connection).clone().ok_or("now playing isn't connected yet")?;
        zbus::block_on(connection.call_method(Some(bus), MPRIS_PATH, Some(interface), method, body))
            .map(|_| ())
            .map_err(|e| format!("{bus} didn't take {method}: {e}"))
    }
}

/// The wire form of one player.
fn wire_player(player: &Player, app: Option<nova_protocol::App>, browser: bool) -> MediaPlayer {
    let reading = &player.reading;
    MediaPlayer {
        id: player.id.clone(),
        app,
        identity: player::text(reading.identity.clone()),
        status: reading.status,
        title: reading.title.clone(),
        artist: reading.artist.clone(),
        album: reading.album.clone(),
        cover: player.cover.as_ref().map(|path| path.to_string_lossy().into_owned()),
        browser,
        length_us: reading.length_us,
        position_us: reading.position_us,
        position_at_ms: player.read_at_ms,
        rate: reading.rate,
        can_next: reading.can_next,
        can_previous: reading.can_previous,
        can_play: reading.can_play,
        can_pause: reading.can_pause,
        can_seek: reading.can_seek,
        can_raise: reading.can_raise,
    }
}

/// The lower-case ids an app's windows may have: its desktop entry's name
/// and its identity, for `find_app` and the compositor.
fn app_ids(reading: &Reading) -> Vec<String> {
    [&reading.desktop_entry, &reading.identity]
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_lowercase())
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0)
}

async fn serve(shared: &Arc<Shared>, bus: Bus, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let connection = bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
    // Followed before the players are listed, so none starting in between is missed.
    let owners = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(DBUS_NAME)
        .interface(DBUS_NAME)
        .member("NameOwnerChanged")
        .build()?;
    let mut owners = MessageStream::for_match_rule(owners, &connection, None).await?;
    let changes = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(PROPERTIES_INTERFACE)
        .member("PropertiesChanged")
        .path(MPRIS_PATH)
        .build()?;
    let mut changes = MessageStream::for_match_rule(changes, &connection, None).await?;
    let seeks = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(PLAYER_INTERFACE)
        .member("Seeked")
        .path(MPRIS_PATH)
        .build()?;
    let mut seeks = MessageStream::for_match_rule(seeks, &connection, None).await?;
    *lock(&shared.connection) = Some(connection.clone());
    let _ = ready.send(());
    for name in list_names(&connection).await? {
        if name.starts_with(MPRIS_PREFIX) {
            add_player(shared, &connection, &name, None).await;
        }
    }
    shared.logger.info(LOG_SOURCE, "following media players");
    let result = follow(shared, &connection, &mut owners, &mut changes, &mut seeks).await;
    *lock(&shared.connection) = None;
    result
}

/// Every name on the bus.
async fn list_names(connection: &Connection) -> zbus::Result<Vec<String>> {
    let reply = connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "ListNames", &()).await?;
    reply.body().deserialize()
}

/// Handles players coming and going, and their changes, until `stop` or the bus goes.
async fn follow(
    shared: &Arc<Shared>,
    connection: &Connection,
    owners: &mut MessageStream,
    changes: &mut MessageStream,
    seeks: &mut MessageStream,
) -> zbus::Result<()> {
    loop {
        let message = poll_fn(|cx| {
            if shared.stop.poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            if let Poll::Ready(message) = Pin::new(&mut *owners).poll_next(cx) {
                return Poll::Ready(message.map(|message| (Source::Owners, message)));
            }
            if let Poll::Ready(message) = Pin::new(&mut *changes).poll_next(cx) {
                return Poll::Ready(message.map(|message| (Source::Changes, message)));
            }
            if let Poll::Ready(message) = Pin::new(&mut *seeks).poll_next(cx) {
                return Poll::Ready(message.map(|message| (Source::Seeks, message)));
            }
            Poll::Pending
        })
        .await;
        let Some((source, message)) = message else { return Ok(()) };
        let message = message?;
        match source {
            Source::Owners => {
                let (name, old, new): (String, String, String) = message.body().deserialize()?;
                if !name.starts_with(MPRIS_PREFIX) {
                    continue;
                }
                match (old.is_empty(), new.is_empty()) {
                    (_, true) => remove_player(shared, &name),
                    (true, false) => add_player(shared, connection, &name, Some(new)).await,
                    (false, false) => remove_player(shared, &name),
                }
            }
            Source::Changes | Source::Seeks => {
                let Some(sender) = message.header().sender().map(|sender| sender.to_string()) else { continue };
                // Only a player's own signals name its interfaces, so a change to another app's properties is ignored.
                if let Some(id) = bus_name_of(shared, &sender) {
                    refresh(shared, connection, &id).await;
                }
            }
        }
    }
}

/// Where a message came from, when the loop has more than one stream.
enum Source {
    Owners,
    Changes,
    Seeks,
}

/// The bus name of the player owned by the unique name `sender`.
fn bus_name_of(shared: &Shared, sender: &str) -> Option<String> {
    let state = lock(&shared.state);
    state.players.iter().find(|player| player.unique == sender).map(|player| player.id.clone())
}

/// Reads a player that just appeared and adds it, unless the list is full.
async fn add_player(shared: &Arc<Shared>, connection: &Connection, id: &str, unique: Option<String>) {
    let unique = match unique {
        Some(unique) => unique,
        None => match owner_of(connection, id).await {
            Some(unique) => unique,
            None => return,
        },
    };
    if lock(&shared.state).players.len() >= MAX_PLAYERS {
        shared.logger.warn(LOG_SOURCE, &format!("ignoring {id}: {MAX_PLAYERS} players is the most"));
        return;
    }
    let Some(reading) = read(connection, id).await else { return };
    let mut state = lock(&shared.state);
    state.players.retain(|player| player.id != id);
    let activated = if reading.status == MediaStatus::Stopped {
        0
    } else {
        state.seq += 1;
        state.seq
    };
    state.players.push(Player {
        id: id.to_string(),
        unique,
        read_at_ms: now_ms(),
        reading: reading.clone(),
        cover: None,
        activated,
    });
    shared.publish(&mut state);
    drop(state);
    fetch_cover(shared, id, &reading.art_url);
}

/// The unique name that owns the bus name `id`.
async fn owner_of(connection: &Connection, id: &str) -> Option<String> {
    let reply =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(id,)).await.ok()?;
    reply.body().deserialize().ok()
}

/// Forgets the player `id`, when it leaves the bus.
fn remove_player(shared: &Shared, id: &str) {
    let mut state = lock(&shared.state);
    let before = state.players.len();
    state.players.retain(|player| player.id != id);
    if state.players.len() != before {
        shared.publish(&mut state);
    }
}

/// Reads a player that changed and updates it.
async fn refresh(shared: &Arc<Shared>, connection: &Connection, id: &str) {
    let Some(reading) = read(connection, id).await else { return };
    let mut state = lock(&shared.state);
    let Some(index) = state.players.iter().position(|player| player.id == id) else { return };
    let previous_art = state.players[index].reading.art_url.clone();
    let was = state.players[index].reading.status;
    if was != reading.status && reading.status != MediaStatus::Stopped {
        state.seq += 1;
        let seq = state.seq;
        state.players[index].activated = seq;
    }
    if previous_art != reading.art_url {
        state.players[index].cover = None;
    }
    let player = &mut state.players[index];
    player.reading = reading.clone();
    player.read_at_ms = now_ms();
    shared.publish(&mut state);
    drop(state);
    if previous_art != reading.art_url {
        fetch_cover(shared, id, &reading.art_url);
    }
}

/// Reads the player `id`'s two property sets; `None` when one fails or
/// doesn't answer in time, so the player keeps what it had.
async fn read(connection: &Connection, id: &str) -> Option<Reading> {
    let root = get_all(connection, id, ROOT_INTERFACE).await?;
    let player = get_all(connection, id, PLAYER_INTERFACE).await?;
    Some(player::read(&root, &player))
}

/// `GetAll` for one interface of the player `id`.
async fn get_all(connection: &Connection, id: &str, interface: &str) -> Option<HashMap<String, OwnedVariant>> {
    let reply =
        connection.call_method(Some(id), MPRIS_PATH, Some(PROPERTIES_INTERFACE), "GetAll", &(interface,)).await.ok()?;
    reply.body().deserialize().ok()
}

/// Gives `id`'s cover to the state: a local file at once, an HTTPS one
/// once it's fetched, on a thread of its own.
fn fetch_cover(shared: &Arc<Shared>, id: &str, art_url: &str) {
    match cover::source(art_url) {
        None => {}
        Some(cover::Source::Local(path)) => set_cover(shared, id, art_url, path),
        Some(cover::Source::Remote(url)) => {
            let (shared, id, art_url) = (Arc::clone(shared), id.to_string(), art_url.to_string());
            std::thread::spawn(move || match cover::fetch(&url, &shared.cache_dir) {
                Ok(path) => set_cover(&shared, &id, &art_url, path),
                Err(e) => shared.logger.warn(LOG_SOURCE, &format!("no cover for {id}: {e}")),
            });
        }
    }
}

/// Sets `id`'s cover, if it still has the art `art_url` it was fetched for.
fn set_cover(shared: &Shared, id: &str, art_url: &str, path: PathBuf) {
    let mut state = lock(&shared.state);
    let Some(player) = state.players.iter_mut().find(|player| player.id == id) else { return };
    if player.reading.art_url != art_url {
        return;
    }
    player.cover = Some(path);
    shared.publish(&mut state);
}
