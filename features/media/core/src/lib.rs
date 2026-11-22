//! Now playing (feature `media`): the MPRIS media players on the session
//! bus, which one the bar shows, and their controls.
//!
//! Players are the bus names under `org.mpris.MediaPlayer2.`, followed with
//! `NameOwnerChanged` (no polling). Each one is read whole (`player`) when it
//! comes and again whenever it says a property changed
//! (`PropertiesChanged`); one player's reads run one at a time on a thread
//! of their own, so a slow player holds up nobody else. `Position` changes
//! without a signal, so each read stamps when it read it and the UI moves it
//! on at the player's rate; `Seeked` says when it jumps. Covers become image
//! files (`cover`). Every player is published as one [`Event::Media`], with
//! the one the bar shows: the user's pick (`media_show`) until another
//! player starts playing or the pick moves on to another track while it
//! isn't playing, else the one that started playing last, else the
//! one paused last.
//!
//! The app a player belongs to is looked up in the app index by its
//! `DesktopEntry`, then by the process that owns it on the bus, as the
//! tray does (the [`ProcessNames`] port novad fills with the tray's
//! lookup): never by its bus name, since browsers
//! name theirs `org.mpris.MediaPlayer2.firefox.instance_NNN`. A player's own
//! desktop entry can be wrong: Zen's says `zen`, its app is
//! `zen-beta.desktop`, and its process is `zen-beta`.

mod cover;
mod player;
#[cfg(test)]
mod tests;

use cover::Covers;
use futures_core::Stream;
use nova_apps::AppIndex;
use nova_base::lock;
use nova_carrier::log::LogLevel;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_media_api::{Event, MEDIA_PLAYER_MAX_BYTES, Media as MediaEvent, MediaAction, MediaPlayer, PlaybackStatus};
use nova_session_bus::{Bus, Stop};
use player::{PATH, PLAYER_INTERFACE, ROOT_INTERFACE, Read};
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use zbus::{Connection, MatchRule, MessageStream};

pub const FEATURE_ID: &str = "media";
const LOG_SOURCE: &str = "media";
/// Every MPRIS player's bus name starts with this.
const PREFIX: &str = "org.mpris.MediaPlayer2.";
/// playerctld's name: it stands in for whichever player played last, so
/// following it too would list that player twice.
const PLAYERCTLD: &str = "org.mpris.MediaPlayer2.playerctld";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// More than any desktop plays at once; players past it are left out.
const MAX_PLAYERS: usize = 32;
/// How long novad waits for a player, or the bus, to answer a call.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Where the players are, and where covers are downloaded to.
#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    pub covers: PathBuf,
    /// Where a player's process is looked up (`/proc`, or a copy in tests).
    pub proc: PathBuf,
    /// The names a player's process goes by.
    pub process_names: ProcessNames,
}

impl Sources {
    /// Covers go in `runtime_dir/media`, beside novad's socket.
    pub fn new(runtime_dir: &Path, process_names: ProcessNames) -> Self {
        Self { bus: Bus::Session, covers: runtime_dir.join("media"), proc: PathBuf::from("/proc"), process_names }
    }
}

/// The names the process `pid` (read from `proc`, normally `/proc`) goes
/// by, to find its app with: a port novad fills with the tray's lookup,
/// which finds a tray icon's app the same way.
pub type ProcessNames = fn(proc: &Path, pid: Option<u32>) -> Vec<String>;

pub struct Media {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    /// Finds a player's app.
    apps: Arc<AppIndex>,
    /// The connection to the bus, once it's up, for the commands.
    connection: Mutex<Option<Connection>>,
    covers: Covers,
    proc: PathBuf,
    process_names: ProcessNames,
    stop: Stop,
}

#[derive(Default)]
struct State {
    players: Vec<Player>,
    /// The player the user picked (`media_show`), until another starts
    /// playing or it moves on to another track while it isn't playing.
    picked: Option<String>,
    /// Counts status changes, so the latest to start playing or pause is known.
    clock: u64,
    /// The last event published, so an unchanged one isn't sent again.
    sent: Option<MediaEvent>,
}

struct Player {
    /// Its well-known bus name.
    id: String,
    /// The unique name that owns it: signals come from this.
    owner: String,
    /// `None` until it's first read.
    read: Option<Read>,
    /// The names its owner's process goes by, to find its app with;
    /// `None` until read with it.
    process: Option<Vec<String>>,
    /// Its cover as a file, and the art URL that file is for.
    cover: Option<String>,
    cover_url: Option<String>,
    /// When `read.position_us` was read or last jumped, in milliseconds since 1970.
    position_at_ms: i64,
    /// `State::clock` when it last started playing, and when it last paused.
    played: u64,
    paused: u64,
    /// A read runs, and whether another was asked for meanwhile.
    reading: bool,
    again: bool,
}

impl Media {
    /// Connects to the bus and follows its players on a thread of its own.
    /// Returns once the players there are known (their reads may still run),
    /// or connecting failed (logged).
    pub fn start(sources: Sources, apps: Arc<AppIndex>, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            hub,
            logger,
            apps,
            connection: Mutex::new(None),
            covers: Covers::new(sources.covers),
            proc: sources.proc,
            process_names: sources.process_names,
            stop: Stop::default(),
        });
        let (ready, started) = std::sync::mpsc::channel();
        let serving = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(serve(&serving, sources.bus, &ready)) {
                serving.logger.warn(LOG_SOURCE, &format!("now playing won't show: {e}"));
            }
            *lock(&serving.connection) = None;
            let _ = ready.send(());
        });
        // Waits as long as connecting may take; a timed-out call fails sooner.
        let _ = started.recv_timeout(2 * CALL_TIMEOUT);
        let media = Arc::new(Self { shared });
        // Looked up again too, for a read of the apps that ended while it
        // connected, before it listened.
        media.shared.apps.on_change(&media, Self::apps_changed);
        media.apps_changed();
        media
    }

    /// Stops following the players and publishes nothing more.
    pub fn stop(&self) {
        let _state = lock(&self.shared.state);
        self.shared.stop.stop();
    }

    /// Looks the players' apps up again, after the installed apps changed.
    fn apps_changed(&self) {
        self.shared.publish(&mut lock(&self.shared.state));
    }

    /// Sends a new subscriber, client `id`, the players as they are, under
    /// the lock that publishing takes, so a newer event can't arrive before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Media(self.shared.event(&state)));
    }

    /// Shows `player` in the bar until another player starts playing.
    pub fn show(&self, player: &str) -> Result<(), String> {
        let mut state = lock(&self.shared.state);
        find(&state, player)?;
        state.picked = Some(player.to_string());
        self.shared.publish(&mut state);
        Ok(())
    }

    /// Plays or pauses `player`, or skips a track.
    pub fn control(&self, player: &str, action: MediaAction) -> Result<(), String> {
        let (owner, _) = self.known(player)?;
        let method = match action {
            MediaAction::PlayPause => "PlayPause",
            MediaAction::Next => "Next",
            MediaAction::Previous => "Previous",
        };
        self.call(&owner, PLAYER_INTERFACE, method, &()).map_err(|e| format!("{player} didn't {method}: {e}"))
    }

    /// Moves `player` to `position_us` into its track, kept within the track.
    pub fn seek(&self, player: &str, position_us: i64) -> Result<(), String> {
        let (owner, read) = self.known(player)?;
        if !read.can_seek {
            return Err(format!("{player} can't seek"));
        }
        let target = position_us.clamp(0, if read.length_us > 0 { read.length_us } else { i64::MAX });
        let failed = |e: zbus::Error| format!("{player} didn't seek: {e}");
        match &read.track_id {
            Some(track) => self.call(&owner, PLAYER_INTERFACE, "SetPosition", &(track, target)).map_err(failed),
            // Without a track id, SetPosition is ignored: move by the difference.
            None => {
                let now = self.position(player)?;
                self.call(&owner, PLAYER_INTERFACE, "Seek", &(target - now,)).map_err(failed)
            }
        }
    }

    /// Asks `player` to bring its window forward (MPRIS `Raise`). False when
    /// it can't.
    pub fn raise(&self, player: &str) -> Result<bool, String> {
        let (owner, read) = self.known(player)?;
        if !read.can_raise {
            return Ok(false);
        }
        self.call(&owner, ROOT_INTERFACE, "Raise", &()).map_err(|e| format!("{player} didn't raise: {e}"))?;
        Ok(true)
    }

    /// The app ids (lower case) `player`'s windows may have: its app's,
    /// after its desktop entry's name; without an app, all its own names
    /// (`names`). Its process's other names can be another app's: one
    /// started from a terminal sits in the terminal's unit.
    pub fn window_ids(&self, player: &str) -> Result<Vec<String>, String> {
        let (names, entry) = {
            let state = lock(&self.shared.state);
            let found = find(&state, player)?;
            let read = found.read.as_ref().ok_or_else(|| format!("{player} hasn't been read yet"))?;
            (names(found), read.desktop_entry.as_deref().map(str::to_lowercase))
        };
        let apps = &self.shared.apps;
        let Some(app_ids) = apps.find_app(&names).and_then(|app| apps.window_ids(&app.id)) else {
            return Ok(names);
        };
        let mut ids: Vec<String> = entry.into_iter().collect();
        for id in app_ids {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// The owner of the read player `player`, and its read.
    fn known(&self, player: &str) -> Result<(String, Read), String> {
        let state = lock(&self.shared.state);
        let found = find(&state, player)?;
        let read = found.read.clone().ok_or_else(|| format!("{player} hasn't been read yet"))?;
        Ok((found.owner.clone(), read))
    }

    /// Where `player` is in its track now.
    fn position(&self, player: &str) -> Result<i64, String> {
        let state = lock(&self.shared.state);
        let found = find(&state, player)?;
        let read = found.read.as_ref().ok_or_else(|| format!("{player} hasn't been read yet"))?;
        if read.status != PlaybackStatus::Playing {
            return Ok(read.position_us);
        }
        let elapsed_us = (now_ms() - found.position_at_ms).max(0) as f64 * 1000.0 * read.rate;
        Ok(read.position_us + elapsed_us as i64)
    }

    fn call<B>(&self, owner: &str, interface: &str, method: &str, body: &B) -> zbus::Result<()>
    where
        B: serde::Serialize + zbus::wire::DynamicType,
    {
        let connection =
            lock(&self.shared.connection).clone().ok_or(zbus::Error::Failure("not on the session bus".into()))?;
        zbus::block_on(connection.call_method(Some(owner), PATH, Some(interface), method, body)).map(drop)
    }
}

/// The names (lower case) to find `player`'s app and windows by: its
/// desktop entry, then its process's names.
fn names(player: &Player) -> Vec<String> {
    let entry = player.read.as_ref().and_then(|read| read.desktop_entry.as_deref()).map(str::to_lowercase);
    let mut names: Vec<String> = entry.into_iter().collect();
    for name in player.process.iter().flatten() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

/// The player `id`, if novad follows it.
fn find<'a>(state: &'a State, id: &str) -> Result<&'a Player, String> {
    if id.len() > MEDIA_PLAYER_MAX_BYTES {
        return Err(format!("player ids are at most {MEDIA_PLAYER_MAX_BYTES} bytes"));
    }
    state.players.iter().find(|player| player.id == id).ok_or_else(|| format!("no player {id:?}"))
}

impl Shared {
    fn event(&self, state: &State) -> MediaEvent {
        let apps = self.apps.read();
        let players = state
            .players
            .iter()
            .filter_map(|player| {
                let read = player.read.as_ref()?;
                let app = apps.find_app(&names(player)).map(|app| (apps.is_browser(&app.id), app));
                let identity = if read.identity.is_empty() { &player.id[PREFIX.len()..] } else { &read.identity };
                Some(MediaPlayer {
                    id: player.id.clone(),
                    browser: app.as_ref().is_some_and(|(browser, _)| *browser),
                    app: app.map(|(_, app)| app),
                    identity: identity.to_string(),
                    status: read.status,
                    title: read.title.clone(),
                    artist: read.artist.clone(),
                    album: read.album.clone(),
                    cover: player.cover.clone(),
                    length_us: read.length_us,
                    position_us: read.position_us,
                    position_at_ms: player.position_at_ms,
                    rate: read.rate,
                    can_next: read.can_next,
                    can_previous: read.can_previous,
                    can_play: read.can_play,
                    can_pause: read.can_pause,
                    can_seek: read.can_seek,
                    can_raise: read.can_raise,
                })
            })
            .collect();
        MediaEvent { players, shown: shown(state) }
    }

    fn publish(&self, state: &mut State) {
        if self.stop.stopped() {
            return;
        }
        let event = self.event(state);
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Media(event.clone()));
            state.sent = Some(event);
        }
    }

    /// Follows the player `id`, owned by `owner`, and reads it.
    fn add(self: &Arc<Self>, connection: &Connection, id: &str, owner: &str) {
        let mut state = lock(&self.state);
        if let Some(player) = state.players.iter_mut().find(|player| player.id == id) {
            // Handed to another connection: that one's the player now.
            player.owner = owner.to_string();
            player.process = None;
        } else if state.players.len() >= MAX_PLAYERS {
            self.logger.warn(LOG_SOURCE, &format!("left {id} out: already following {MAX_PLAYERS} players"));
            return;
        } else {
            self.logger.info(LOG_SOURCE, &format!("{id} came"));
            state.players.push(Player {
                id: id.to_string(),
                owner: owner.to_string(),
                read: None,
                process: None,
                cover: None,
                cover_url: None,
                position_at_ms: 0,
                played: 0,
                paused: 0,
                reading: false,
                again: false,
            });
        }
        self.read_soon(&mut state, connection, id);
    }

    fn remove(&self, id: &str) {
        let mut state = lock(&self.state);
        let before = state.players.len();
        state.players.retain(|player| player.id != id);
        // The user's pick ends with the player: one back under its name
        // later is a new one.
        if state.picked.as_deref() == Some(id) {
            state.picked = None;
        }
        if state.players.len() != before {
            self.logger.info(LOG_SOURCE, &format!("{id} went"));
            self.publish(&mut state);
        }
    }

    /// Reads the players `owner` owns again: they said something changed.
    fn changed(self: &Arc<Self>, connection: &Connection, owner: &str) {
        let mut state = lock(&self.state);
        let ids: Vec<String> =
            state.players.iter().filter(|player| player.owner == owner).map(|player| player.id.clone()).collect();
        for id in ids {
            self.read_soon(&mut state, connection, &id);
        }
    }

    /// The players `owner` owns jumped to `position_us`.
    fn seeked(&self, owner: &str, position_us: i64) {
        let mut state = lock(&self.state);
        for player in state.players.iter_mut().filter(|player| player.owner == owner) {
            if let Some(read) = &mut player.read {
                read.position_us = position_us.max(0);
                player.position_at_ms = now_ms();
            }
        }
        self.publish(&mut state);
    }

    /// Reads the player `id` on a thread of its own, or once more after the
    /// read that runs now.
    fn read_soon(self: &Arc<Self>, state: &mut State, connection: &Connection, id: &str) {
        let Some(player) = state.players.iter_mut().find(|player| player.id == id) else { return };
        if player.reading {
            player.again = true;
            return;
        }
        player.reading = true;
        let (shared, connection, id) = (Arc::clone(self), connection.clone(), id.to_string());
        std::thread::spawn(move || {
            loop {
                shared.read(&connection, &id);
                let mut state = lock(&shared.state);
                let Some(player) = state.players.iter_mut().find(|player| player.id == id) else { return };
                if !player.again || shared.stop.stopped() {
                    player.reading = false;
                    return;
                }
                player.again = false;
            }
        });
    }

    /// Reads the player `id` and publishes it; a new cover follows once
    /// downloaded. A player that went, or was handed to another connection,
    /// meanwhile is left as it is.
    fn read(self: &Arc<Self>, connection: &Connection, id: &str) {
        let Some((owner, known)) = lock(&self.state)
            .players
            .iter()
            .find(|player| player.id == id)
            .map(|player| (player.owner.clone(), player.process.is_some()))
        else {
            return;
        };
        let read = match zbus::block_on(player::read(connection, &owner)) {
            Ok(read) => read,
            Err(e) => {
                self.logger.log(LogLevel::Debug, LOG_SOURCE, &format!("can't read {id}: {e}"));
                return;
            }
        };
        let process = (!known).then(|| self.process_names(connection, id, &owner));
        let read_at_ms = now_ms();
        let url = read.art_url.clone();
        let mut state = lock(&self.state);
        let State { players, picked, clock, .. } = &mut *state;
        let Some(player) = players.iter_mut().find(|player| player.id == id && player.owner == owner) else { return };
        let was = player.read.as_ref().map(|read| read.status);
        // A picked player that isn't playing moving to another track ends
        // the pick too: a browser keeps one player for all its tabs, so
        // closing the picked tab leaves it there with another tab's media,
        // or none.
        let moved_on = player.read.as_ref().is_some_and(|old| old.track_id != read.track_id || old.title != read.title);
        if moved_on && read.status != PlaybackStatus::Playing && picked.as_deref() == Some(id) {
            *picked = None;
        }
        if was != Some(read.status) {
            *clock += 1;
            match read.status {
                PlaybackStatus::Playing => {
                    player.played = *clock;
                    // Another player starting ends the user's pick.
                    if picked.as_deref().is_some_and(|picked| picked != id) {
                        *picked = None;
                    }
                }
                PlaybackStatus::Paused => player.paused = *clock,
                PlaybackStatus::Stopped => {}
            }
        }
        player.read = Some(read);
        if let Some(process) = process {
            player.process = Some(process);
        }
        player.position_at_ms = read_at_ms;
        let new_cover = player.cover_url != url;
        if new_cover {
            player.cover = None;
            player.cover_url = None;
        }
        match url {
            // Downloaded on a thread of its own, so a slow cover holds back
            // neither this read nor the player's next ones. Its URL counts
            // as handled meanwhile, so it's downloaded once.
            Some(url) if new_cover && Covers::downloads(&url) => {
                player.cover_url = Some(url.clone());
                self.publish(&mut state);
                drop(state);
                let (shared, id, owner) = (Arc::clone(self), id.to_string(), owner);
                std::thread::spawn(move || shared.download_cover(&id, &owner, &url));
            }
            // A local one only needs a look, so it comes with the rest.
            Some(url) if new_cover => {
                player.cover = self.cover(id, &url);
                player.cover_url = Some(url);
                self.publish(&mut state);
            }
            _ => self.publish(&mut state),
        }
    }

    /// The names the process that owns `owner`, player `id`'s bus name,
    /// goes by; none when the bus can't tell which process it is.
    fn process_names(&self, connection: &Connection, id: &str, owner: &str) -> Vec<String> {
        let pid = zbus::block_on(connection.call_method(
            Some(DBUS_NAME),
            DBUS_PATH,
            Some(DBUS_NAME),
            "GetConnectionUnixProcessID",
            &(owner,),
        ))
        .and_then(|reply| reply.body().deserialize::<u32>());
        if let Err(e) = &pid {
            self.logger.log(LogLevel::Debug, LOG_SOURCE, &format!("can't tell which process {id} is in: {e}"));
        }
        (self.process_names)(&self.proc, pid.ok())
    }

    /// Downloads player `id`'s cover at `url`, and publishes it unless the
    /// player has moved on meanwhile.
    fn download_cover(&self, id: &str, owner: &str, url: &str) {
        let cover = self.cover(id, url);
        if self.stop.stopped() {
            return;
        }
        let mut state = lock(&self.state);
        let Some(player) = state
            .players
            .iter_mut()
            .find(|player| player.id == id && player.owner == owner && player.cover_url.as_deref() == Some(url))
        else {
            return;
        };
        player.cover = cover;
        self.publish(&mut state);
    }

    /// The file for player `id`'s cover at `url`, if it has one.
    fn cover(&self, id: &str, url: &str) -> Option<String> {
        match self.covers.file(url) {
            Ok(file) => file.map(|file| file.display().to_string()),
            Err(e) => {
                self.logger.info(LOG_SOURCE, &format!("{id}'s cover won't show: {e}"));
                None
            }
        }
    }
}

/// The player the bar shows: the user's pick while it isn't stopped, else
/// the one that started playing last, else the one paused last.
fn shown(state: &State) -> Option<String> {
    let status = |player: &Player| player.read.as_ref().map(|read| read.status);
    let picked = state.picked.as_ref().and_then(|picked| state.players.iter().find(|player| &player.id == picked));
    let latest = |wanted: PlaybackStatus, when: fn(&Player) -> u64| {
        state.players.iter().filter(|player| status(player) == Some(wanted)).max_by_key(|player| when(player))
    };
    picked
        .filter(|player| status(player).is_some_and(|status| status != PlaybackStatus::Stopped))
        .or_else(|| latest(PlaybackStatus::Playing, |player| player.played))
        .or_else(|| latest(PlaybackStatus::Paused, |player| player.paused))
        .map(|player| player.id.clone())
}

/// Connects to `bus` and follows its players until `stop` or the bus goes.
/// Sends on `ready` once the players there are known.
async fn serve(shared: &Arc<Shared>, bus: Bus, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let connection = bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
    // Followed before the players are listed, so none that comes or changes
    // in between is missed.
    let signal = |sender: Option<&'static str>, interface: &'static str, member: &'static str| {
        let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface).member(member);
        match sender {
            Some(sender) => rule.sender(sender),
            None => rule.path(PATH),
        }
        .build()
    };
    let mut owners =
        MessageStream::for_match_rule(signal(Some(DBUS_NAME), DBUS_NAME, "NameOwnerChanged")?, &connection, None)
            .await?;
    let mut changes =
        MessageStream::for_match_rule(signal(None, PROPERTIES, "PropertiesChanged")?, &connection, None).await?;
    let mut seeks = MessageStream::for_match_rule(signal(None, PLAYER_INTERFACE, "Seeked")?, &connection, None).await?;
    *lock(&shared.connection) = Some(connection.clone());

    let names: Vec<String> = connection
        .call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "ListNames", &())
        .await?
        .body()
        .deserialize()?;
    for name in names.into_iter().filter(|name| is_player(name)) {
        match name_owner(&connection, &name).await {
            Ok(owner) => shared.add(&connection, &name, &owner),
            Err(e) => shared.logger.log(LogLevel::Debug, LOG_SOURCE, &format!("{name} has no owner: {e}")),
        }
    }
    shared.logger.info(LOG_SOURCE, "following media players");
    let _ = ready.send(());

    loop {
        let next = poll_fn(|cx| {
            if shared.stop.poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            for (kind, stream) in
                [(Signal::Owner, &mut owners), (Signal::Changed, &mut changes), (Signal::Seeked, &mut seeks)]
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
        let sender = message.header().sender().map(|sender| sender.to_string()).unwrap_or_default();
        match kind {
            Signal::Owner => {
                let (name, _old, new): (String, String, String) = message.body().deserialize()?;
                if !is_player(&name) {
                    continue;
                }
                if new.is_empty() {
                    shared.remove(&name);
                } else {
                    shared.add(&connection, &name, &new);
                }
            }
            Signal::Changed => {
                let Ok((interface, ..)) = message.body().deserialize::<(
                    String,
                    std::collections::HashMap<String, zbus::wire::OwnedVariant>,
                    Vec<String>,
                )>() else {
                    continue;
                };
                if interface == PLAYER_INTERFACE || interface == ROOT_INTERFACE {
                    shared.changed(&connection, &sender);
                }
            }
            Signal::Seeked => {
                if let Ok(position) = message.body().deserialize::<i64>() {
                    shared.seeked(&sender, position);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Owner,
    Changed,
    Seeked,
}

async fn name_owner(connection: &Connection, name: &str) -> zbus::Result<String> {
    let reply = connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(name,)).await?;
    reply.body().deserialize()
}

/// Whether bus name `name` is a media player of its own.
fn is_player(name: &str) -> bool {
    name.starts_with(PREFIX) && name != PLAYERCTLD
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
