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
const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYERCTLD: &str = "org.mpris.MediaPlayer2.playerctld";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const MAX_PLAYERS: usize = 32;
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const SHORT_CLIP: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    pub covers: PathBuf,
    pub proc: PathBuf,
    pub process_names: ProcessNames,
}

impl Sources {
    pub fn new(runtime_dir: &Path, process_names: ProcessNames) -> Self {
        Self { bus: Bus::Session, covers: runtime_dir.join("media"), proc: PathBuf::from("/proc"), process_names }
    }
}

pub type ProcessNames = fn(proc: &Path, pid: Option<u32>) -> Vec<String>;

pub struct Media {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    apps: Arc<AppIndex>,
    connection: Mutex<Option<Connection>>,
    covers: Covers,
    proc: PathBuf,
    process_names: ProcessNames,
    stop: Stop,
}

#[derive(Default)]
struct State {
    players: Vec<Player>,
    picked: Option<String>,
    clock: u64,
    sent: Option<MediaEvent>,
}

struct Player {
    id: String,
    owner: String,
    read: Option<Read>,
    process: Option<Vec<String>>,
    cover: Option<String>,
    cover_url: Option<String>,
    position_at_ms: i64,
    played: u64,
    paused: u64,
    reading: bool,
    again: bool,
}

impl Media {
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
        let _ = started.recv_timeout(2 * CALL_TIMEOUT);
        let media = Arc::new(Self { shared });
        media.shared.apps.on_change(&media, Self::apps_changed);
        media.apps_changed();
        media
    }

    pub fn stop(&self) {
        let _state = lock(&self.shared.state);
        self.shared.stop.stop();
    }

    fn apps_changed(&self) {
        self.shared.publish(&mut lock(&self.shared.state));
    }

    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Media(self.shared.event(&state)));
    }

    pub fn show(&self, player: &str) -> Result<(), String> {
        let mut state = lock(&self.shared.state);
        find(&state, player)?;
        state.picked = Some(player.to_string());
        self.shared.publish(&mut state);
        Ok(())
    }

    pub fn control(&self, player: &str, action: MediaAction) -> Result<(), String> {
        let (owner, _) = self.known(player)?;
        let method = match action {
            MediaAction::PlayPause => "PlayPause",
            MediaAction::Next => "Next",
            MediaAction::Previous => "Previous",
        };
        self.call(&owner, PLAYER_INTERFACE, method, &()).map_err(|e| format!("{player} didn't {method}: {e}"))
    }

    pub fn seek(&self, player: &str, position_us: i64) -> Result<(), String> {
        let (owner, read) = self.known(player)?;
        if !read.can_seek {
            return Err(format!("{player} can't seek"));
        }
        let target = position_us.clamp(0, if read.length_us > 0 { read.length_us } else { i64::MAX });
        let failed = |e: zbus::Error| format!("{player} didn't seek: {e}");
        match &read.track_id {
            Some(track) => self.call(&owner, PLAYER_INTERFACE, "SetPosition", &(track, target)).map_err(failed),
            None => {
                let now = self.position(player)?;
                self.call(&owner, PLAYER_INTERFACE, "Seek", &(target - now,)).map_err(failed)
            }
        }
    }

    pub fn raise(&self, player: &str) -> Result<bool, String> {
        let (owner, read) = self.known(player)?;
        if !read.can_raise {
            return Ok(false);
        }
        self.call(&owner, ROOT_INTERFACE, "Raise", &()).map_err(|e| format!("{player} didn't raise: {e}"))?;
        Ok(true)
    }

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

    fn known(&self, player: &str) -> Result<(String, Read), String> {
        let state = lock(&self.shared.state);
        let found = find(&state, player)?;
        let read = found.read.clone().ok_or_else(|| format!("{player} hasn't been read yet"))?;
        Ok((found.owner.clone(), read))
    }

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

fn find<'a>(state: &'a State, id: &str) -> Result<&'a Player, String> {
    if id.len() > MEDIA_PLAYER_MAX_BYTES {
        return Err(format!("player ids are at most {MEDIA_PLAYER_MAX_BYTES} bytes"));
    }
    let player = state.players.iter().find(|player| player.id == id).ok_or_else(|| format!("no player {id:?}"))?;
    match player.read.as_ref().is_some_and(short_clip) {
        true => Err(format!("{id} plays a short clip, which isn't listed")),
        false => Ok(player),
    }
}

fn short_clip(read: &Read) -> bool {
    read.length_us > 0 && read.length_us < SHORT_CLIP.as_micros() as i64
}

impl Shared {
    fn event(&self, state: &State) -> MediaEvent {
        let apps = self.apps.read();
        let players = state
            .players
            .iter()
            .filter_map(|player| {
                let read = player.read.as_ref().filter(|read| !short_clip(read))?;
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

    fn add(self: &Arc<Self>, connection: &Connection, id: &str, owner: &str) {
        let mut state = lock(&self.state);
        if let Some(player) = state.players.iter_mut().find(|player| player.id == id) {
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
        if state.picked.as_deref() == Some(id) {
            state.picked = None;
        }
        if state.players.len() != before {
            self.logger.info(LOG_SOURCE, &format!("{id} went"));
            self.publish(&mut state);
        }
    }

    fn changed(self: &Arc<Self>, connection: &Connection, owner: &str) {
        let mut state = lock(&self.state);
        let ids: Vec<String> =
            state.players.iter().filter(|player| player.owner == owner).map(|player| player.id.clone()).collect();
        for id in ids {
            self.read_soon(&mut state, connection, &id);
        }
    }

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
        let moved_on = player.read.as_ref().is_some_and(|old| old.track_id != read.track_id || old.title != read.title);
        if moved_on && read.status != PlaybackStatus::Playing && picked.as_deref() == Some(id) {
            *picked = None;
        }
        if was != Some(read.status) {
            *clock += 1;
            match read.status {
                PlaybackStatus::Playing => {
                    player.played = *clock;
                    if picked.as_deref().is_some_and(|picked| picked != id) && !short_clip(&read) {
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
            Some(url) if new_cover && Covers::downloads(&url) => {
                player.cover_url = Some(url.clone());
                self.publish(&mut state);
                drop(state);
                let (shared, id, owner) = (Arc::clone(self), id.to_string(), owner);
                std::thread::spawn(move || shared.download_cover(&id, &owner, &url));
            }
            Some(url) if new_cover => {
                player.cover = self.cover(id, &url);
                player.cover_url = Some(url);
                self.publish(&mut state);
            }
            _ => self.publish(&mut state),
        }
    }

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

fn shown(state: &State) -> Option<String> {
    let status = |player: &Player| player.read.as_ref().filter(|read| !short_clip(read)).map(|read| read.status);
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

async fn serve(shared: &Arc<Shared>, bus: Bus, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let connection = bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
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

fn is_player(name: &str) -> bool {
    name.starts_with(PREFIX) && name != PLAYERCTLD
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
