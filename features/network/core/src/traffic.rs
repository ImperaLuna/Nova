//! Traffic over the machine's wired and Wi-Fi links: bytes per second for
//! the network page's graph, and today's totals.
//!
//! One thread reads the kernel's counters (`/sys/class/net/<link>/
//! statistics/{rx,tx}_bytes`) of the links NetworkManager lists: every
//! [`NETWORK_TRAFFIC_INTERVAL_SECS`] while some connection watches
//! (`watch_network_traffic`), publishing each look, and every
//! [`UNWATCHED_INTERVAL`] otherwise, so today's totals and the last
//! minute's history (coarser then) are there when the page opens. A
//! counter that went down (its link was made again) counts from zero.
//! Today's totals are saved to the state folder now and then and when the
//! service stops, and start again at local midnight.

use nova_base::lock;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_network_api::{Event, NETWORK_TRAFFIC_HISTORY_SECS, NETWORK_TRAFFIC_INTERVAL_SECS, NetworkTraffic};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const LOG_SOURCE: &str = "network";
const WATCHED_INTERVAL: Duration = Duration::from_secs(NETWORK_TRAFFIC_INTERVAL_SECS);
/// How often counters are read while nobody watches.
const UNWATCHED_INTERVAL: Duration = Duration::from_secs(5);
/// How many steps the history holds.
const HISTORY_STEPS: usize = (NETWORK_TRAFFIC_HISTORY_SECS / NETWORK_TRAFFIC_INTERVAL_SECS) as usize;
/// How often today's totals are saved while they change.
const SAVE_INTERVAL: Duration = Duration::from_secs(60);

/// Where counters are read and today's totals kept.
#[derive(Clone)]
pub struct Paths {
    /// `/sys`.
    pub sys_root: PathBuf,
    /// The saved totals, in the state folder.
    pub totals: PathBuf,
}

pub struct Traffic {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    paths: Paths,
}

struct State {
    watchers: BTreeSet<ClientId>,
    /// The links to count, from NetworkManager.
    links: Vec<String>,
    /// Each link's counters (down, up) at the last look.
    counters: HashMap<String, (u64, u64)>,
    last_look: Option<Instant>,
    down: VecDeque<u64>,
    up: VecDeque<u64>,
    today: Totals,
    saved: Totals,
    last_save: Instant,
    /// A watcher came since the sampler last looked at the state.
    greet: bool,
    stopped: bool,
}

/// Today's bytes, as saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Totals {
    /// The local date they're for, like `2026-10-31`.
    date: String,
    down: u64,
    up: u64,
}

impl Traffic {
    pub fn start(paths: Paths, hub: Arc<Hub>, logger: Arc<Logger>) -> Self {
        let today = load(&paths.totals, &logger)
            .filter(|totals| totals.date == local_date())
            .unwrap_or(Totals { date: local_date(), ..Totals::default() });
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                watchers: BTreeSet::new(),
                links: Vec::new(),
                counters: HashMap::new(),
                last_look: None,
                down: VecDeque::new(),
                up: VecDeque::new(),
                saved: today.clone(),
                today,
                last_save: Instant::now(),
                greet: false,
                stopped: false,
            }),
            wake: Condvar::new(),
            hub,
            logger,
            paths,
        });
        let sampler = Arc::clone(&shared);
        std::thread::spawn(move || sampler.sample());
        Self { shared }
    }

    /// The links NetworkManager lists now. One that comes is counted from
    /// its next look on.
    pub fn set_links(&self, mut links: Vec<String>) {
        links.sort();
        links.dedup();
        lock(&self.shared.state).links = links;
    }

    pub fn watch(&self, client: ClientId, watching: bool) {
        let mut state = lock(&self.shared.state);
        let changed = match watching {
            true => state.watchers.insert(client),
            false => state.watchers.remove(&client),
        };
        if changed && watching {
            state.greet = true;
            self.shared.wake.notify_all();
        }
    }

    /// Stops sampling and saves today's totals.
    pub fn stop(&self) {
        let mut state = lock(&self.shared.state);
        state.stopped = true;
        self.shared.save(&mut state);
        self.shared.wake.notify_all();
    }
}

impl Shared {
    fn sample(&self) {
        let mut state = lock(&self.state);
        loop {
            if state.stopped {
                return;
            }
            let now = Instant::now();
            // A new watcher gets what's there at once when the last look is
            // too recent for a new one; else the look below is due.
            if std::mem::take(&mut state.greet) && state.last_look.is_some_and(|last| now - last < WATCHED_INTERVAL) {
                self.publish(&state);
            }
            let interval = if state.watchers.is_empty() { UNWATCHED_INTERVAL } else { WATCHED_INTERVAL };
            let due = state.last_look.map_or(now, |last| last + interval);
            if now < due {
                state = self.wake.wait_timeout(state, due - now).unwrap_or_else(std::sync::PoisonError::into_inner).0;
                continue;
            }
            self.look(&mut state, now);
            if !state.watchers.is_empty() {
                self.publish(&state);
            }
            if now - state.last_save >= SAVE_INTERVAL {
                self.save(&mut state);
            }
        }
    }

    fn look(&self, state: &mut State, now: Instant) {
        let date = local_date();
        if state.today.date != date {
            state.today = Totals { date, ..Totals::default() };
        }
        let (mut down, mut up) = (0u64, 0u64);
        let mut counters = HashMap::new();
        for link in &state.links {
            let Some(read) = counters_of(&self.paths.sys_root, link) else { continue };
            if let Some(before) = state.counters.get(link) {
                down += grown(before.0, read.0);
                up += grown(before.1, read.1);
            }
            counters.insert(link.clone(), read);
        }
        state.counters = counters;
        state.today.down += down;
        state.today.up += up;
        let elapsed = state.last_look.map(|last| now - last);
        state.last_look = Some(now);
        let Some(elapsed) = elapsed.filter(|elapsed| !elapsed.is_zero()) else { return };
        // One history step per interval that passed, each at the average rate.
        let steps = ((elapsed.as_secs_f64() / WATCHED_INTERVAL.as_secs_f64()).round() as usize).clamp(1, HISTORY_STEPS);
        let per_second = |bytes: u64| (bytes as f64 / elapsed.as_secs_f64()).round() as u64;
        for _ in 0..steps {
            push(&mut state.down, per_second(down));
            push(&mut state.up, per_second(up));
        }
    }

    fn publish(&self, state: &State) {
        self.hub.publish(&Event::NetworkTraffic(NetworkTraffic {
            down: state.down.iter().copied().collect(),
            up: state.up.iter().copied().collect(),
            today_down: state.today.down,
            today_up: state.today.up,
        }));
    }

    fn save(&self, state: &mut State) {
        state.last_save = Instant::now();
        if state.today == state.saved {
            return;
        }
        match nova_base::state_file::save(&self.paths.totals, &state.today) {
            Ok(()) => state.saved = state.today.clone(),
            Err(e) => self.logger.warn(LOG_SOURCE, &format!("can't save today's traffic: {e}")),
        }
    }
}

fn push(history: &mut VecDeque<u64>, value: u64) {
    if history.len() == HISTORY_STEPS {
        history.pop_front();
    }
    history.push_back(value);
}

/// How far a counter moved; from zero when it went back (the link was made
/// again under the same name).
fn grown(before: u64, now: u64) -> u64 {
    now.checked_sub(before).unwrap_or(now)
}

/// A link's counters (down, up), `None` when it isn't there.
fn counters_of(sys_root: &Path, link: &str) -> Option<(u64, u64)> {
    // Names come from NetworkManager; one that would leave the folder isn't a link.
    if link.is_empty() || link.contains('/') || link.starts_with('.') {
        return None;
    }
    let statistics = sys_root.join("class/net").join(link).join("statistics");
    let read = |name: &str| std::fs::read_to_string(statistics.join(name)).ok()?.trim().parse::<u64>().ok();
    Some((read("rx_bytes")?, read("tx_bytes")?))
}

fn local_date() -> String {
    jiff::Zoned::now().date().to_string()
}

fn load(path: &Path, logger: &Logger) -> Option<Totals> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("can't read today's traffic: {e}"));
            return None;
        }
    };
    serde_json::from_str(&text)
        .map_err(|e| logger.warn(LOG_SOURCE, &format!("today's traffic doesn't parse, starting from zero: {e}")))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_counter_that_went_back_counts_from_zero() {
        assert_eq!(grown(100, 250), 150);
        assert_eq!(grown(1_000, 40), 40);
    }

    #[test]
    fn links_outside_the_folder_are_never_read() {
        let dir = std::env::temp_dir();
        assert_eq!(counters_of(&dir, "../etc"), None);
        assert_eq!(counters_of(&dir, ""), None);
    }
}
