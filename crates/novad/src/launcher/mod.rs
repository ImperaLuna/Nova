//! The launcher (feature `launcher`): searching the installed apps (from
//! the shared app index, `crate::apps`), starting them, and the user's
//! pinned and recent ones, which survive restarts. It publishes the apps
//! again each time the index reads them.

pub mod autostart;
pub mod providers;
mod rank;

use crate::apps::AppIndex;
use crate::apps::desktop_entry::Entry;
pub use crate::apps::launch::Runner;
use crate::compositor::Compositor;
use crate::hub::Hub;
use crate::logger::Logger;
use crate::tray::Tray;
use crate::{lock, state_file};
use nova_protocol::{App, Event, LauncherItem};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FEATURE_ID: &str = "launcher";
const LOG_SOURCE: &str = "launcher";
/// Matches a provider returns for a search, at most.
pub const MAX_MATCHES: usize = 50;
/// Apps that can be pinned at once.
const MAX_PINNED: usize = 32;
/// Recent apps sent, the latest first.
const MAX_RECENT: usize = 8;
/// Apps whose launches are remembered (for recent and for ranking); the
/// least recently launched are forgotten first.
const MAX_REMEMBERED: usize = 200;
/// How long novad waits to focus the window an app was just asked to show
/// (from its tray icon or menu, or a click on its notification), and how
/// often it looks.
pub const SHOWN_WINDOW_WAIT: Duration = if cfg!(test) { Duration::from_secs(1) } else { Duration::from_secs(3) };
const TRAY_WINDOW_RETRY: Duration = Duration::from_millis(100);
/// How long it waits for the window of an app in the tray that it started
/// again: the new process first starts up, then hands over to the running
/// one.
pub const STARTED_WINDOW_WAIT: Duration = if cfg!(test) { Duration::from_secs(2) } else { Duration::from_secs(10) };

/// Where the module keeps things, and how it starts apps.
#[derive(Clone)]
pub struct Paths {
    /// Pinned apps and launch counts.
    pub state_file: PathBuf,
    pub runner: Runner,
}

impl Paths {
    pub fn new(state_dir: &Path, path_var: &str) -> Self {
        Self { state_file: state_dir.join("launcher.json"), runner: Runner::detect(path_var) }
    }
}

pub struct Launcher {
    state: Mutex<State>,
    apps: Arc<AppIndex>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    paths: Paths,
    /// Where programs are looked for, and the `PATH` apps get.
    path_var: String,
    /// Set by `stop`.
    stopped: AtomicBool,
}

struct State {
    saved: Saved,
    /// The last event published, so an unchanged list isn't sent again.
    sent: Option<nova_protocol::Launcher>,
}

/// What survives a restart.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    /// Ids in the user's order. Apps that are gone stay, in case they come back.
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    launches: BTreeMap<String, Launches>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Launches {
    count: u32,
    /// The last one, in milliseconds since 1970.
    last: i64,
}

impl Launcher {
    /// Loads the saved state, and publishes the apps whenever `apps` reads
    /// them (now, if it already has).
    pub fn start(paths: Paths, apps: Arc<AppIndex>, path_var: &str, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let saved = match std::fs::read_to_string(&paths.state_file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", paths.state_file.display()));
                Saved::default()
            }),
            Err(_) => Saved::default(),
        };
        let launcher = Arc::new(Self {
            state: Mutex::new(State { saved, sent: None }),
            apps,
            hub,
            logger,
            paths,
            path_var: path_var.to_string(),
            stopped: AtomicBool::new(false),
        });
        launcher.apps.on_change(&launcher, Self::apps_changed);
        launcher.apps_changed();
        launcher
    }

    /// Publishes nothing more.
    pub fn stop(&self) {
        let _state = lock(&self.state);
        self.stopped.store(true, Ordering::SeqCst);
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Publishes the apps once the index has read them.
    fn apps_changed(&self) {
        let mut state = lock(&self.state);
        if !self.stopped() && self.apps.read().scanned() {
            self.publish(&mut state);
        }
    }

    /// Sends a new subscriber, client `id`, the apps as they are, under the lock that publishing
    /// takes, so a newer state published meanwhile can't arrive before it.
    pub fn send_state(&self, id: crate::hub::ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(id, &Event::Launcher(snapshot(&state, &self.apps.read())));
    }

    /// The apps matching `query`, best first, as the apps provider's items
    /// (`providers`). A blank query matches nothing.
    pub fn search_apps(&self, query: &str) -> Vec<LauncherItem> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let state = lock(&self.state);
        let apps = self.apps.read();
        let mut found: Vec<(u32, &Entry, Vec<nova_protocol::TextRun>)> = apps
            .entries()
            .values()
            .filter_map(|entry| {
                let launches = state.saved.launches.get(&entry.id).map_or(0, |launches| launches.count);
                rank::score(entry, &query, launches).map(|(score, name)| (score, entry, name))
            })
            .collect();
        found.sort_by(|(a_score, a, _), (b_score, b, _)| {
            b_score
                .cmp(a_score)
                .then(a.name.chars().count().cmp(&b.name.chars().count()))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });
        found
            .into_iter()
            .take(MAX_MATCHES)
            .map(|(_, entry, name)| {
                let app = apps.app_of(entry);
                LauncherItem {
                    provider: nova_protocol::PROVIDER_APPS.to_string(),
                    id: app.id,
                    name,
                    description: app.description,
                    icon: app.icon,
                    file: None,
                }
            })
            .collect()
    }

    /// Focuses a window the app `id` already has, through `focus` (given the
    /// app ids its windows may have, none for a terminal app; `false` when
    /// none is open), or else starts the app; either counts it as launched.
    pub fn launch(&self, id: &str, focus: impl FnOnce(&[String]) -> Result<bool, String>) -> Result<(), String> {
        let ids = self.window_ids(id).ok_or_else(|| format!("no app {id:?}"))?;
        if self.stopped() {
            return Err(crate::features::not_ready(FEATURE_ID));
        }
        let focused = focus(&ids).unwrap_or_else(|e| {
            self.logger.warn(LOG_SOURCE, &format!("the compositor can't help with {id}, so starting it: {e}"));
            false
        });
        if !focused {
            // Focusing may have taken a while; `start_app` checks for a stop.
            self.start_app(id)?;
        }
        let now = jiff::Timestamp::now().as_millisecond();
        self.update(|saved| {
            let launches = saved.launches.entry(id.to_string()).or_insert(Launches { count: 0, last: now });
            launches.count = launches.count.saturating_add(1);
            launches.last = now;
            if saved.launches.len() > MAX_REMEMBERED {
                let mut by_last: Vec<(i64, String)> =
                    saved.launches.iter().map(|(id, launches)| (launches.last, id.clone())).collect();
                by_last.sort();
                for (_, id) in by_last.into_iter().take(saved.launches.len() - MAX_REMEMBERED) {
                    saved.launches.remove(&id);
                }
            }
            Ok(())
        })
    }

    /// Starts the app `id`, without counting it as launched (`launch`
    /// does that).
    pub fn start_app(&self, id: &str) -> Result<(), String> {
        let entry = self.apps.entry(id).ok_or_else(|| format!("no app {id:?}"))?;
        if self.stopped() {
            return Err(crate::features::not_ready(FEATURE_ID));
        }
        crate::apps::launch::launch(&entry, &self.paths.runner, &self.path_var, &self.logger)
            .inspect_err(|e| self.logger.warn(LOG_SOURCE, &format!("can't launch {id}: {e}")))
    }

    /// The app ids the windows of the installed app `id` may have; `None`
    /// when no such app is installed.
    pub fn window_ids(&self, id: &str) -> Option<Vec<String>> {
        self.apps.window_ids(id)
    }

    /// Pins the app `id` at the end, or unpins it.
    pub fn pin(&self, id: &str, pinned: bool) -> Result<(), String> {
        if pinned && !self.apps.read().entries().contains_key(id) {
            return Err(format!("no app {id:?}"));
        }
        self.update(|saved| {
            let at = saved.pinned.iter().position(|pin| pin == id);
            match (pinned, at) {
                (true, None) if saved.pinned.len() >= MAX_PINNED => {
                    Err(format!("at most {MAX_PINNED} apps can be pinned; unpin one first"))
                }
                (true, None) => {
                    saved.pinned.push(id.to_string());
                    Ok(())
                }
                (false, Some(at)) => {
                    saved.pinned.remove(at);
                    Ok(())
                }
                (true, Some(_)) | (false, None) => Ok(()),
            }
        })
    }

    /// Applies `change` to the saved state, then saves and publishes it.
    fn update(&self, change: impl FnOnce(&mut Saved) -> Result<(), String>) -> Result<(), String> {
        let mut state = lock(&self.state);
        if self.stopped() {
            return Err(crate::features::not_ready(FEATURE_ID));
        }
        let before = state.saved.clone();
        change(&mut state.saved)?;
        if state.saved == before {
            return Ok(());
        }
        // Under the lock, so changes reach the file in the order they were made.
        if let Err(e) = state_file::save(&self.paths.state_file, &state.saved) {
            self.logger.warn(LOG_SOURCE, &format!("can't save {}: {e}", self.paths.state_file.display()));
        }
        self.publish(&mut state);
        Ok(())
    }

    fn publish(&self, state: &mut State) {
        let event = snapshot(state, &self.apps.read());
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Launcher(event.clone()));
            state.sent = Some(event);
        }
    }
}

/// The event for `state` and `apps`: apps by name, and the pinned and
/// recent ones that are installed (recent leaves the pinned ones out).
fn snapshot(state: &State, apps: &crate::apps::State) -> nova_protocol::Launcher {
    let installed = apps.entries();
    let mut listed: Vec<App> = installed.values().map(|entry| apps.app_of(entry)).collect();
    listed.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    let is_installed = |id: &&String| installed.contains_key(*id);
    let pinned = state.saved.pinned.iter().filter(is_installed).cloned().collect();
    // A pinned app already shows among the pins.
    let mut launched: Vec<(&String, &Launches)> = state
        .saved
        .launches
        .iter()
        .filter(|(id, _)| installed.contains_key(*id) && !state.saved.pinned.contains(id))
        .collect();
    launched.sort_by(|a, b| b.1.last.cmp(&a.1.last).then_with(|| a.0.cmp(b.0)));
    let recent = launched.into_iter().take(MAX_RECENT).map(|(id, _)| id.clone()).collect();
    nova_protocol::Launcher { apps: listed, pinned, recent }
}
/// The focus step of [`Launcher::launch`] for app `id`, whose windows may
/// have the app ids `ids` (none, and no `tray`, when a new window is asked
/// for): an open window takes focus where it is; a new one opens on
/// `output` (the launcher's screen), which the compositor puts it on once
/// focused. An app that only sits in the tray shows its window from there,
/// as starting it again may just quit (Discord). One whose icon can't show it (Claude
/// Desktop ignores `Activate`, libayatana's icons have only a menu) is
/// started again with `start`: a running app then shows its window, which
/// gets no focus from the compositor either (the activation token went to
/// the process that handed over), so it takes focus here once it shows.
/// `true` when nothing (more) needs starting.
pub fn focus_open(
    id: &str,
    ids: &[String],
    compositor: Option<&Compositor>,
    tray: Option<&Tray>,
    output: Option<&str>,
    start: impl FnOnce() -> Result<(), String>,
    logger: &Logger,
) -> Result<bool, String> {
    if let Some(compositor) = compositor {
        if !ids.is_empty() && compositor.focus_app(ids, output)? {
            return Ok(true);
        }
        if let Some(output) = output {
            compositor.focus_output(output)?;
        }
    }
    let Some(tray) = tray.filter(|tray| tray.has_app(id)) else { return Ok(false) };
    let activated = tray.activate_app(id).unwrap_or_else(|e| {
        logger.warn(LOG_SOURCE, &e);
        false
    });
    let shown = |wait| compositor.is_none_or(|compositor| focus_shown(id, ids, compositor, output, wait, logger));
    if activated && shown(SHOWN_WINDOW_WAIT) {
        return Ok(true);
    }
    start()?;
    shown(STARTED_WINDOW_WAIT);
    Ok(true)
}

/// Focuses the window of app `id` (with one of the app ids `ids`) that it
/// was just asked to show (by its tray icon, its menu, a click on its
/// notification, or starting it again while it runs), once it shows: niri
/// gives it no focus, as none of those hands the app an activation token.
/// Gives up (logged) after `wait`.
/// `false` when no window showed; `true` also when that can't be told (an
/// app in a terminal, the compositor failing).
pub fn focus_shown(
    id: &str,
    ids: &[String],
    compositor: &Compositor,
    output: Option<&str>,
    wait: Duration,
    logger: &Logger,
) -> bool {
    if ids.is_empty() {
        return true;
    }
    let deadline = std::time::Instant::now() + wait;
    loop {
        match compositor.focus_app(ids, output) {
            Ok(true) => return true,
            Ok(false) if std::time::Instant::now() < deadline => std::thread::sleep(TRAY_WINDOW_RETRY),
            Ok(false) => {
                logger.info(LOG_SOURCE, &format!("{id}'s window didn't show"));
                return false;
            }
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("can't focus {id}'s window: {e}"));
                return true;
            }
        }
    }
}

/// It has no options: it only starts and stops with its feature.
impl crate::modules::Module for Launcher {
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_protocol::Topic::Launcher);
    type Sources = Paths;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Paths {
        Paths::new(&env.args.state_dir, env.path_var)
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, paths: &Paths, (): &(), _: &[nova_protocol::FeatureState]) -> Arc<Self> {
        Self::start(paths.clone(), Arc::clone(&ctx.apps), &ctx.path_var, Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))
    }

    fn send_state(&self, client: crate::hub::ClientId) {
        Self::send_state(self, client);
    }

    fn stop(&self) {
        Self::stop(self);
    }
}
