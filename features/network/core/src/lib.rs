mod actions;
#[cfg(test)]
mod fake_nm;
mod manager;
#[cfg(test)]
mod tests;
mod traffic;

use futures_core::Stream;
use manager::{ACTIVE, ACTIVE_STATE_ACTIVATED, NM, Reading};
use nova_base::lock;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_network_api::{
    AttemptOutcome, Event, LinkKind, LinkState, Network as NetworkEvent, Wifi, WifiAttempt, WifiNetwork, WifiPassword,
    WiredLink,
};
use nova_session_bus::{Bus, Stop};
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
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(250);
const MAX_DELAY: Duration = Duration::from_secs(1);
const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_NETWORKS: usize = 64;
const MAX_NAME_CHARS: usize = 100;
const MAX_TRANSITIONS: usize = 32;
const PREFERENCE_TRIES: u32 = 3;
const DEFAULT_METRIC_WIRED: i64 = 100;
const DEFAULT_METRIC_WIFI: i64 = 600;

#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    pub sys_root: PathBuf,
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
    wake: Condvar,
    connection: Mutex<Option<Connection>>,
    stop: Stop,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    traffic: traffic::Traffic,
    prefer_file: PathBuf,
}

struct State {
    reading: Option<Reading>,
    stale: Option<(Instant, Instant)>,
    transitions: VecDeque<(OwnedObjectPath, u32, u32)>,
    attempt: Option<Attempt>,
    scan: Option<Scan>,
    prefer: Option<LinkKind>,
    preferred_on: Option<(OwnedObjectPath, OwnedObjectPath, LinkKind)>,
    last_preference: Option<(OwnedObjectPath, OwnedObjectPath, LinkKind)>,
    preference_tries: u32,
    sent: NetworkEvent,
    stopped: bool,
}

struct Attempt {
    ssid: String,
    active: OwnedObjectPath,
    created: Option<OwnedObjectPath>,
    last: Option<(u32, u32)>,
}

struct Scan {
    last_scan: i64,
    asked: Instant,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    prefer: Option<LinkKind>,
}

impl Network {
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
        self.shared.wake.notify_all();
        Ok(())
    }

    pub fn connect(&self, ssid: &str, password: Option<&WifiPassword>) -> Result<(), String> {
        actions::validate_ssid(ssid)?;
        let (connection, reading) = self.shared.now()?;
        let started = zbus::block_on(actions::connect(&connection, &reading, ssid, password))?;
        self.shared.logger.info(LOG_SOURCE, &format!("connecting to {ssid}"));
        let mut state = lock(&self.shared.state);
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
        nova_base::state_file::save(&self.shared.prefer_file, &Saved { prefer: Some(kind) })
            .map_err(|e| format!("can't save the preferred link: {e}"))?;
        state.prefer = Some(kind);
        state.preferred_on = None;
        self.shared.publish(&mut state);
        self.shared.mark_stale(&mut state);
        Ok(())
    }
}

impl Shared {
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

    async fn follow(&self, bus: &Bus) -> zbus::Result<()> {
        let connection = bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
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
                let ready =
                    [&mut owners, &mut signals].into_iter().find_map(|stream| match Pin::new(stream).poll_next(cx) {
                        Poll::Ready(message) => Some(message),
                        Poll::Pending => None,
                    });
                ready.map_or(Poll::Pending, |message| Poll::Ready(Some(message)))
            })
            .await;
            let Some(Some(message)) = next else { return Ok(()) };
            self.heard(&message?);
        }
    }

    fn heard(&self, message: &zbus::Message) {
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
            if let Some(attempt) = state.attempt.as_mut().filter(|attempt| attempt.active.as_str() == path.as_str()) {
                attempt.last = Some((active_state, reason));
                self.publish(&mut state);
            }
        }
        self.mark_stale(&mut state);
    }

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
            drop(state);
            let reading = zbus::block_on(manager::read(&connection));
            state = lock(&self.state);
            if state.stopped {
                return;
            }
            let reading = match reading {
                Ok(reading) => Some(reading),
                Err(e) => {
                    if state.reading.is_some() {
                        self.logger.info(LOG_SOURCE, &format!("NetworkManager stopped answering: {e}"));
                    }
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
            if failed.is_some() && state.preference_tries < PREFERENCE_TRIES {
                state.preference_tries += 1;
                state.preferred_on = None;
                let now = Instant::now();
                state.stale.get_or_insert((now, now));
            }
        }
    }

    fn wrong_password_profile(&self, state: &mut State) -> Option<OwnedObjectPath> {
        let outcome = outcome(state)?;
        let attempt = state.attempt.as_mut()?;
        (outcome == AttemptOutcome::WrongPassword).then(|| attempt.created.take()).flatten()
    }

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

    fn publish(&self, state: &mut State) {
        let event = event(state);
        if event != state.sent {
            self.hub.publish(&Event::Network(event.clone()));
            state.sent = event;
        }
    }
}

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

fn connected_pair(reading: &Reading) -> Option<(&manager::WiredDevice, &manager::WifiDevice)> {
    let wired = reading.wired.iter().find(|device| device.state == LinkState::Connected && device.active.is_some())?;
    let wifi = reading.wifi.as_ref().filter(|wifi| wifi.state == LinkState::Connected && wifi.active.is_some())?;
    Some((wired, wifi))
}

fn links(reading: &Reading) -> Vec<String> {
    reading
        .wired
        .iter()
        .map(|device| device.interface.clone())
        .chain(reading.wifi.as_ref().map(|wifi| wifi.interface.clone()))
        .collect()
}

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

fn targets(picked: i64, other: i64, picked_saved: Option<i64>, other_saved: Option<i64>) -> (Option<i64>, Option<i64>) {
    if picked < other {
        (picked_saved, other_saved)
    } else if other > 0 {
        (Some(other - 1), other_saved)
    } else {
        (Some(0), Some(1))
    }
}

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
