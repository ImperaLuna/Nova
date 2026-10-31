//! Wired and Wi-Fi in quick settings (feature `network`), from
//! NetworkManager on the system bus: what each link is doing, the Wi-Fi
//! networks in reach, connecting to one, Wi-Fi's switch, which link to
//! prefer while both are connected, and the traffic (`traffic`).
//!
//! Event-driven: one thread follows every signal NetworkManager sends
//! (properties of its objects changing, devices and access points coming
//! and going, connections changing state) and `NameOwnerChanged` for it
//! starting or stopping. Signals only mark the state stale; another thread
//! reads everything again once they pause (`manager::read`, a scan sends
//! dozens at once) and publishes one [`Event::Network`] when it differs from
//! the last. NetworkManager's own state is never cached past a read.
//!
//! A `connect_wifi` is followed by its active connection's `StateChanged`
//! signals: activated is connected; deactivated for missing secrets or a
//! failed login is a wrong password (and the profile novad made for a new
//! network is removed again), anything else a failure.
//!
//! The preferred link (`prefer_network`) is the user's pick, saved in the
//! state folder. While both links are connected and the other one carries
//! traffic, novad sets their route metrics on the devices (`Reapply`), not
//! in the saved profiles, which may be the system's (NixOS declares them):
//! the picked one's goes just under the other's.

mod actions;
#[cfg(test)]
mod fake_nm;
mod manager;
#[cfg(test)]
mod tests;
mod traffic;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::session_bus::{Bus, Stop};
use futures_core::Stream;
use manager::{ACTIVE, ACTIVE_STATE_ACTIVATED, NM, Reading};
use nova_protocol::{
    AttemptOutcome, Event, LinkKind, LinkState, Network as NetworkEvent, Wifi, WifiAttempt, WifiNetwork, WifiPassword,
    WiredLink,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};
use zbus::wire::OwnedObjectPath;
use zbus::{Connection, MatchRule, MessageStream};

pub const FEATURE_ID: &str = "network";
const LOG_SOURCE: &str = "network";
const DBUS_NAME: &str = "org.freedesktop.DBus";
/// How long novad waits for the bus or NetworkManager to answer a call.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Signals that come together are read once they pause this long...
const SETTLE: Duration = Duration::from_millis(250);
/// ...or this long after the first, if they never pause.
const MAX_DELAY: Duration = Duration::from_secs(1);
/// A scan that hasn't finished by then isn't shown as running any more.
const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
/// The most networks listed: more than any list needs.
const MAX_NETWORKS: usize = 64;
/// Connection names are cut to this many characters.
const MAX_NAME_CHARS: usize = 100;
/// How many connections' last `StateChanged` are kept.
const MAX_TRANSITIONS: usize = 32;
/// NetworkManager's default route metrics, for a profile that sets none.
/// How many times a failed preference is tried again before it waits for
/// the links or the pick to change.
const PREFERENCE_TRIES: u32 = 3;
const DEFAULT_METRIC_WIRED: i64 = 100;
const DEFAULT_METRIC_WIFI: i64 = 600;

/// Where the network is read from: the machine's, or stand-ins in tests.
#[derive(Clone)]
pub struct Sources {
    /// The bus NetworkManager is on.
    pub bus: Bus,
    /// `/sys`, for the links' traffic counters.
    pub sys_root: PathBuf,
    /// Where the preferred link and today's traffic are kept.
    pub state_dir: PathBuf,
}

impl Sources {
    pub fn system(state_dir: &Path) -> Self {
        Self { bus: Bus::System, sys_root: "/sys".into(), state_dir: state_dir.to_owned() }
    }
}

pub struct Network {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    /// Wakes the reader when the state went stale, or it should stop.
    wake: Condvar,
    /// The follower's connection, once it has one.
    connection: Mutex<Option<Connection>>,
    stop: Stop,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    traffic: traffic::Traffic,
    /// The saved preferred link.
    prefer_file: PathBuf,
}

struct State {
    /// What NetworkManager had at the last read, `None` while it isn't
    /// running (or wasn't read yet).
    reading: Option<Reading>,
    /// When the first and the last signal not yet read came.
    stale: Option<(Instant, Instant)>,
    /// The latest `StateChanged` (state, reason) of active connections, for
    /// an attempt whose first ones come before `connect` has its path.
    transitions: VecDeque<(OwnedObjectPath, u32, u32)>,
    attempt: Option<Attempt>,
    scan: Option<Scan>,
    prefer: Option<LinkKind>,
    /// The connections (wired, Wi-Fi) the preference was last set on, and
    /// for which pick: it's set once per pair.
    preferred_on: Option<(OwnedObjectPath, OwnedObjectPath, LinkKind)>,
    /// The pair and pick last tried, and how many times setting it failed.
    last_preference: Option<(OwnedObjectPath, OwnedObjectPath, LinkKind)>,
    preference_tries: u32,
    sent: NetworkEvent,
    stopped: bool,
}

struct Attempt {
    ssid: String,
    active: OwnedObjectPath,
    /// The profile novad made, until it's removed.
    created: Option<OwnedObjectPath>,
    /// Its active connection's latest (state, reason), kept here so newer
    /// transitions of other connections can't push it out.
    last: Option<(u32, u32)>,
}

struct Scan {
    /// The card's `LastScan` when the scan was asked for: it's done once
    /// that moves.
    last_scan: i64,
    asked: Instant,
}

/// The saved pick, in `network.json`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    prefer: Option<LinkKind>,
}

impl Network {
    /// Starts following NetworkManager and counting traffic. Returns at
    /// once: the state is published as it's read.
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let prefer_file = sources.state_dir.join("network.json");
        let prefer = load_prefer(&prefer_file, &logger);
        let traffic = traffic::Traffic::start(
            traffic::Paths { sys_root: sources.sys_root, totals: sources.state_dir.join("network-traffic.json") },
            Arc::clone(&hub),
            Arc::clone(&logger),
        );
        let sent = NetworkEvent { running: false, primary: None, prefer: None, wired: vec![], wifi: None };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                reading: None,
                stale: None,
                transitions: VecDeque::new(),
                attempt: None,
                scan: None,
                prefer,
                preferred_on: None,
                last_preference: None,
                preference_tries: 0,
                sent: sent.clone(),
                stopped: false,
            }),
            wake: Condvar::new(),
            connection: Mutex::new(None),
            stop: Stop::default(),
            hub,
            logger,
            traffic,
            prefer_file,
        });
        // Clients that heard an earlier run start from nothing again.
        shared.hub.publish(&Event::Network(sent));
        let follower = Arc::clone(&shared);
        let bus = sources.bus;
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(follower.follow(&bus)) {
                follower.logger.info(LOG_SOURCE, &format!("not following NetworkManager: {e}"));
            }
        });
        let reader = Arc::clone(&shared);
        std::thread::spawn(move || reader.read_when_settled());
        Arc::new(Self { shared })
    }

    pub fn stop(&self) {
        lock(&self.shared.state).stopped = true;
        self.shared.wake.notify_all();
        self.shared.stop.stop();
        self.shared.traffic.stop();
    }

    /// Sends a new subscriber, client `id`, the state last published, under
    /// the lock that publishing takes, so a newer event can't arrive before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Network(state.sent.clone()));
    }

    pub fn watch_traffic(&self, client: ClientId, watching: bool) {
        self.shared.traffic.watch(client, watching);
    }

    pub fn set_wifi(&self, enabled: bool) -> Result<(), String> {
        let (connection, reading) = self.shared.now()?;
        zbus::block_on(actions::set_wifi(&connection, &reading, enabled))
    }

    pub fn scan(&self) -> Result<(), String> {
        let (connection, reading) = self.shared.now()?;
        let last_scan = reading.wifi.as_ref().ok_or("there's no Wi-Fi card")?.last_scan;
        zbus::block_on(actions::scan(&connection, &reading))?;
        let mut state = lock(&self.shared.state);
        state.scan = Some(Scan { last_scan, asked: Instant::now() });
        self.shared.publish(&mut state);
        // Woken at the timeout, in case the scan never ends.
        self.shared.wake.notify_all();
        Ok(())
    }

    pub fn connect(&self, ssid: &str, password: Option<&WifiPassword>) -> Result<(), String> {
        actions::validate_ssid(ssid)?;
        let (connection, reading) = self.shared.now()?;
        let started = zbus::block_on(actions::connect(&connection, &reading, ssid, password))?;
        self.shared.logger.info(LOG_SOURCE, &format!("connecting to {ssid}"));
        let mut state = lock(&self.shared.state);
        // Its first state changes may have come before the reply.
        let last = state
            .transitions
            .iter()
            .rev()
            .find(|(path, ..)| *path == started.active)
            .map(|(_, active_state, reason)| (*active_state, *reason));
        state.attempt = Some(Attempt { ssid: ssid.to_owned(), active: started.active, created: started.created, last });
        self.shared.publish(&mut state);
        Ok(())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        let (connection, reading) = self.shared.now()?;
        zbus::block_on(actions::disconnect(&connection, &reading))
    }

    pub fn forget(&self, ssid: &str) -> Result<(), String> {
        actions::validate_ssid(ssid)?;
        let (connection, reading) = self.shared.now()?;
        zbus::block_on(actions::forget(&connection, &reading, ssid))?;
        self.shared.logger.info(LOG_SOURCE, &format!("forgot {ssid}"));
        Ok(())
    }

    pub fn prefer(&self, kind: LinkKind) -> Result<(), String> {
        if kind == LinkKind::Other {
            return Err("only a wired or a Wi-Fi link can be preferred".into());
        }
        let mut state = lock(&self.shared.state);
        crate::state_file::save(&self.shared.prefer_file, &Saved { prefer: Some(kind) })
            .map_err(|e| format!("can't save the preferred link: {e}"))?;
        state.prefer = Some(kind);
        state.preferred_on = None;
        self.shared.publish(&mut state);
        // Read again, which sets the route metrics.
        self.shared.mark_stale(&mut state);
        Ok(())
    }
}

impl Shared {
    /// The connection and the last reading, for a command.
    fn now(&self) -> Result<(Connection, Reading), String> {
        let not_running = || "NetworkManager isn't running".to_string();
        let connection = lock(&self.connection).clone().ok_or_else(not_running)?;
        let reading = lock(&self.state).reading.clone().ok_or_else(not_running)?;
        Ok((connection, reading))
    }

    fn mark_stale(&self, state: &mut State) {
        let now = Instant::now();
        state.stale = Some(state.stale.map_or((now, now), |(first, _)| (first, now)));
        self.wake.notify_all();
    }

    /// Follows NetworkManager's signals until stopped or the bus goes.
    async fn follow(&self, bus: &Bus) -> zbus::Result<()> {
        let connection = bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
        // Followed before the first read, so nothing in between is missed.
        let owner_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(DBUS_NAME)
            .interface(DBUS_NAME)
            .member("NameOwnerChanged")
            .arg(0, NM)
            .build()?;
        let mut owners = MessageStream::for_match_rule(owner_rule, &connection, None).await?;
        let nm_rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(NM).build()?;
        let mut signals = MessageStream::for_match_rule(nm_rule, &connection, None).await?;
        *lock(&self.connection) = Some(connection);
        self.mark_stale(&mut lock(&self.state));
        loop {
            let next = poll_fn(|cx| {
                if self.stop.poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                for stream in [&mut owners, &mut signals] {
                    if let Poll::Ready(message) = Pin::new(stream).poll_next(cx) {
                        return Poll::Ready(Some(message));
                    }
                }
                Poll::Pending
            })
            .await;
            let Some(Some(message)) = next else { return Ok(()) };
            let message = message?;
            let header = message.header();
            let mut state = lock(&self.state);
            if header.interface().is_some_and(|interface| interface.as_str() == ACTIVE)
                && header.member().is_some_and(|member| member.as_str() == "StateChanged")
                && let (Some(path), Ok((active_state, reason))) =
                    (header.path(), message.body().deserialize::<(u32, u32)>())
            {
                if state.transitions.len() == MAX_TRANSITIONS {
                    state.transitions.pop_front();
                }
                state.transitions.push_back((path.to_owned().into(), active_state, reason));
                // The outcome shows at once, without waiting for a read.
                if let Some(attempt) = state.attempt.as_mut().filter(|attempt| attempt.active.as_str() == path.as_str())
                {
                    attempt.last = Some((active_state, reason));
                    self.publish(&mut state);
                }
            }
            self.mark_stale(&mut state);
        }
    }

    /// Reads NetworkManager again once signals settle, until stopped.
    fn read_when_settled(&self) {
        let mut state = lock(&self.state);
        loop {
            if state.stopped {
                return;
            }
            let scan_ends = state.scan.as_ref().map(|scan| scan.asked + SCAN_TIMEOUT);
            let read_due = state.stale.map(|(first, last)| (last + SETTLE).min(first + MAX_DELAY));
            let Some(due) = [scan_ends, read_due].into_iter().flatten().min() else {
                state = self.wake.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
                continue;
            };
            let now = Instant::now();
            if now < due {
                state = self.wake.wait_timeout(state, due - now).unwrap_or_else(std::sync::PoisonError::into_inner).0;
                continue;
            }
            if scan_ends.is_some_and(|ends| ends <= now) {
                state.scan = None;
            }
            if read_due.is_none_or(|due| due > now) {
                self.publish(&mut state);
                continue;
            }
            state.stale = None;
            let Some(connection) = lock(&self.connection).clone() else { continue };
            // Read unlocked: commands and signals go on meanwhile.
            drop(state);
            let reading = zbus::block_on(manager::read(&connection));
            state = lock(&self.state);
            // Stopped meanwhile: a new instance may be running, and this one
            // mustn't publish over it or call NetworkManager for it.
            if state.stopped {
                return;
            }
            let reading = match reading {
                Ok(reading) => Some(reading),
                Err(e) => {
                    if state.reading.is_some() {
                        self.logger.info(LOG_SOURCE, &format!("NetworkManager stopped answering: {e}"));
                    }
                    // Its active connections went with it: an attempt's
                    // last state change won't come.
                    state.attempt = None;
                    None
                }
            };
            if state.reading.is_none() && reading.is_some() {
                self.logger.info(LOG_SOURCE, "reading NetworkManager");
            }
            state.reading = reading;
            if let Some(reading) = &state.reading {
                self.traffic.set_links(links(reading));
            }
            self.publish(&mut state);
            // What follows from it, unlocked: calls to NetworkManager.
            let forget = self.wrong_password_profile(&mut state);
            let preference = self.preference_due(&mut state);
            drop(state);
            let stopped = || lock(&self.state).stopped;
            if let Some(profile) = forget
                && !stopped()
                && let Err(e) = zbus::block_on(actions::forget_created(&connection, &profile))
            {
                self.logger.warn(LOG_SOURCE, &format!("can't remove the profile of a network it couldn't join: {e}"));
            }
            let failed = preference.and_then(|(reading, pick)| {
                let e = zbus::block_on(apply_preference(&connection, &reading, pick, &stopped)).err()?;
                self.logger.warn(LOG_SOURCE, &format!("can't prefer the {pick:?} link: {e}"));
                Some(())
            });
            state = lock(&self.state);
            // A failed apply (often a profile that changed under it) is tried
            // again after a fresh read, a few times.
            if failed.is_some() && state.preference_tries < PREFERENCE_TRIES {
                state.preference_tries += 1;
                state.preferred_on = None;
                let now = Instant::now();
                state.stale.get_or_insert((now, now));
            }
        }
    }

    /// The profile novad made for the attempt, once its password turned
    /// out wrong: it's removed, so the network isn't kept as saved.
    fn wrong_password_profile(&self, state: &mut State) -> Option<OwnedObjectPath> {
        let outcome = outcome(state)?;
        let attempt = state.attempt.as_mut()?;
        (outcome == AttemptOutcome::WrongPassword).then(|| attempt.created.take()).flatten()
    }

    /// The reading and pick to set route metrics for, when both links are
    /// connected with connections the preference wasn't set on yet.
    fn preference_due(&self, state: &mut State) -> Option<(Reading, LinkKind)> {
        let pick = state.prefer?;
        let reading = state.reading.as_ref()?;
        let (wired, wifi) = connected_pair(reading)?;
        let key = (wired.active.clone()?, wifi.active.clone()?, pick);
        if state.preferred_on.as_ref() == Some(&key) {
            return None;
        }
        if state.last_preference.as_ref() != Some(&key) {
            state.preference_tries = 0;
        }
        state.last_preference = Some(key.clone());
        state.preferred_on = Some(key);
        Some((reading.clone(), pick))
    }

    /// Publishes the event when it changed.
    fn publish(&self, state: &mut State) {
        let event = event(state);
        if event != state.sent {
            self.hub.publish(&Event::Network(event.clone()));
            state.sent = event;
        }
    }
}

/// The event for the state now.
fn event(state: &State) -> NetworkEvent {
    let Some(reading) = &state.reading else {
        return NetworkEvent { running: false, primary: None, prefer: None, wired: vec![], wifi: None };
    };
    let wired = reading
        .wired
        .iter()
        .map(|device| WiredLink {
            interface: device.interface.clone(),
            state: device.state,
            connection: device.connection.as_deref().map(name),
            speed_mbps: device.speed_mbps,
        })
        .collect();
    let wifi = reading.wifi.as_ref().map(|device| Wifi {
        interface: device.interface.clone(),
        enabled: device.enabled,
        blocked: device.blocked,
        state: device.state,
        network: device.network.as_deref().map(name),
        speed_mbps: device.speed_mbps,
        scanning: state.scan.as_ref().is_some_and(|scan| scan.last_scan == device.last_scan),
        networks: networks(reading),
        attempt: state.attempt.as_ref().map(|attempt| WifiAttempt {
            ssid: attempt.ssid.clone(),
            outcome: outcome(state).unwrap_or(AttemptOutcome::Connecting),
        }),
    });
    let carrying = connected_pair(reading).and(reading.primary).filter(|kind| *kind != LinkKind::Other);
    NetworkEvent { running: true, primary: reading.primary, prefer: state.prefer.or(carrying), wired, wifi }
}

/// How the attempt went, by its connection's last `StateChanged`.
fn outcome(state: &State) -> Option<AttemptOutcome> {
    let attempt = state.attempt.as_ref()?;
    let Some((active_state, reason)) = attempt.last else {
        return Some(AttemptOutcome::Connecting);
    };
    Some(match active_state {
        ACTIVE_STATE_ACTIVATED => AttemptOutcome::Connected,
        _ if actions::wrong_password(active_state, reason) => AttemptOutcome::WrongPassword,
        manager::ACTIVE_STATE_DEACTIVATED => AttemptOutcome::Failed,
        _ => AttemptOutcome::Connecting,
    })
}

/// The networks in reach: each name once, from its strongest access point
/// (the connected one, while it's connected), connected first, then the
/// strongest.
fn networks(reading: &Reading) -> Vec<WifiNetwork> {
    let Some(wifi) = &reading.wifi else { return Vec::new() };
    let mut networks: Vec<(Vec<u8>, WifiNetwork)> = Vec::new();
    for point in wifi.access_points.iter().filter(|point| !point.ssid.is_empty()) {
        let network = WifiNetwork {
            ssid: manager::ssid_text(&point.ssid),
            strength: point.strength,
            security: point.security,
            band: point.band,
            saved: reading
                .profiles
                .iter()
                .any(|profile| profile.kind == manager::TYPE_WIFI && profile.ssid.as_deref() == Some(&point.ssid)),
            connected: point.active && wifi.state == LinkState::Connected,
        };
        match networks.iter_mut().find(|(ssid, _)| *ssid == point.ssid) {
            Some((_, known)) => {
                let better = (network.connected, network.strength) > (known.connected, known.strength);
                if better {
                    *known = network;
                }
            }
            None => networks.push((point.ssid.clone(), network)),
        }
    }
    let mut networks: Vec<WifiNetwork> = networks.into_iter().map(|(_, network)| network).collect();
    networks.sort_by(|a, b| {
        b.connected.cmp(&a.connected).then(b.strength.cmp(&a.strength)).then_with(|| a.ssid.cmp(&b.ssid))
    });
    networks.truncate(MAX_NETWORKS);
    networks
}

/// The first connected wired link and the Wi-Fi card, while both are
/// connected.
fn connected_pair(reading: &Reading) -> Option<(&manager::WiredDevice, &manager::WifiDevice)> {
    let wired = reading.wired.iter().find(|device| device.state == LinkState::Connected && device.active.is_some())?;
    let wifi = reading.wifi.as_ref().filter(|wifi| wifi.state == LinkState::Connected && wifi.active.is_some())?;
    Some((wired, wifi))
}

/// The links whose traffic is counted.
fn links(reading: &Reading) -> Vec<String> {
    reading
        .wired
        .iter()
        .map(|device| device.interface.clone())
        .chain(reading.wifi.as_ref().map(|wifi| wifi.interface.clone()))
        .collect()
}

/// Sets both links' route metrics so the picked one carries traffic, IPv4
/// and IPv6 each on its own: each keeps its profile's metric when that
/// already does it, else the picked one's goes just under the other's.
/// Stops short once `stopped` says the service stopped meanwhile: a new
/// instance may be setting them.
async fn apply_preference(
    connection: &Connection,
    reading: &Reading,
    pick: LinkKind,
    stopped: &dyn Fn() -> bool,
) -> zbus::Result<()> {
    let Some((wired, wifi)) = connected_pair(reading) else { return Ok(()) };
    let wired_link = Link::read(connection, &wired.path, wired.active.as_ref(), DEFAULT_METRIC_WIRED).await?;
    let wifi_link = Link::read(connection, &wifi.path, wifi.active.as_ref(), DEFAULT_METRIC_WIFI).await?;
    let (picked, other) = match pick {
        LinkKind::Wifi => (wifi_link, wired_link),
        _ => (wired_link, wifi_link),
    };
    let mut picked_metrics = [None; 2];
    let mut other_metrics = [None; 2];
    for family in 0..manager::FAMILIES.len() {
        (picked_metrics[family], other_metrics[family]) =
            targets(picked.saved_metric(family), other.saved_metric(family), picked.saved[family], other.saved[family]);
    }
    for (link, metrics) in [(other, other_metrics), (picked, picked_metrics)] {
        let changes = (0..manager::FAMILIES.len())
            .any(|family| link.applied_metric(family) != metrics[family].unwrap_or(link.default_metric));
        if changes && !stopped() {
            actions::set_route_metric(connection, &link.device, link.applied, link.version, metrics).await?;
        }
    }
    Ok(())
}

/// The route metrics (picked, other) that make the picked link carry
/// traffic, from their profiles' effective metrics and what the profiles
/// set (`None`: NetworkManager's default).
fn targets(picked: i64, other: i64, picked_saved: Option<i64>, other_saved: Option<i64>) -> (Option<i64>, Option<i64>) {
    if picked < other {
        (picked_saved, other_saved)
    } else if other > 0 {
        (Some(other - 1), other_saved)
    } else {
        (Some(0), Some(1))
    }
}

/// One link's metrics, per [`manager::FAMILIES`]: its profile's and what's
/// applied now.
struct Link {
    device: OwnedObjectPath,
    applied: manager::Settings,
    version: u64,
    saved: [Option<i64>; 2],
    default_metric: i64,
}

impl Link {
    async fn read(
        connection: &Connection,
        device: &OwnedObjectPath,
        active: Option<&OwnedObjectPath>,
        default_metric: i64,
    ) -> zbus::Result<Self> {
        let (applied, version) = actions::applied(connection, device).await?;
        let profile = match active {
            Some(active) => manager::get_all(connection, active.as_str(), ACTIVE)
                .await?
                .get("Connection")
                .and_then(|value| OwnedObjectPath::try_from(value.try_clone().ok()?).ok()),
            None => None,
        };
        let saved = match profile {
            Some(profile) => {
                let settings = manager::settings(connection, &profile).await?;
                manager::FAMILIES.map(|family| manager::route_metric(&settings, family))
            }
            None => [None; 2],
        };
        Ok(Self { device: device.clone(), applied, version, saved, default_metric })
    }

    fn saved_metric(&self, family: usize) -> i64 {
        self.saved[family].unwrap_or(self.default_metric)
    }

    fn applied_metric(&self, family: usize) -> i64 {
        manager::route_metric(&self.applied, manager::FAMILIES[family]).unwrap_or(self.default_metric)
    }
}

/// A connection's name from NetworkManager: trimmed, cut to [`MAX_NAME_CHARS`].
fn name(text: &str) -> String {
    text.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect()
}

fn load_prefer(path: &Path, logger: &Logger) -> Option<LinkKind> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("can't read the preferred link: {e}"));
            return None;
        }
    };
    match serde_json::from_str::<Saved>(&text) {
        Ok(saved) => saved.prefer,
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("the saved preferred link doesn't parse: {e}"));
            None
        }
    }
}
