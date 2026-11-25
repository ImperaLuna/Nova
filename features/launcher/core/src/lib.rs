pub mod autostart;
pub mod providers;
mod rank;

use nova_app_info::App;
use nova_apps::AppIndex;
use nova_apps::desktop_entry::Entry;
pub use nova_apps::launch::Runner;
use nova_base::{lock, state_file};
use nova_compositor::Compositor;
use nova_hub::Hub;
pub use nova_launcher_api::MAX_MATCHES;
use nova_launcher_api::{Event, LauncherItem};
use nova_log::Logger;
use providers::Run;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FEATURE_ID: &str = "launcher";
const LOG_SOURCE: &str = "launcher";
const MAX_PINNED: usize = 32;
const MAX_RECENT: usize = 8;
const MAX_REMEMBERED: usize = 200;
pub const SHOWN_WINDOW_WAIT: Duration =
    if cfg!(any(test, feature = "test-support")) { Duration::from_secs(1) } else { Duration::from_secs(3) };
const TRAY_WINDOW_RETRY: Duration = Duration::from_millis(100);
pub const STARTED_WINDOW_WAIT: Duration =
    if cfg!(any(test, feature = "test-support")) { Duration::from_secs(2) } else { Duration::from_secs(10) };

pub trait TrayApps: Send + Sync {
    fn in_tray(&self, app_id: &str) -> bool;
    fn activate_in_tray(&self, app_id: &str) -> Result<bool, String>;
}

#[derive(Clone)]
pub struct Paths {
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
    path_var: String,
    stopped: AtomicBool,
}

struct State {
    saved: Saved,
    sent: Option<nova_launcher_api::Launcher>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    launches: BTreeMap<String, Launches>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Launches {
    count: u32,
    last: i64,
}

impl Launcher {
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

    pub fn stop(&self) {
        let _state = lock(&self.state);
        self.stopped.store(true, Ordering::SeqCst);
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn apps_changed(&self) {
        let mut state = lock(&self.state);
        if !self.stopped() && self.apps.read().scanned() {
            self.publish(&mut state);
        }
    }

    pub fn send_state(&self, id: nova_hub::ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(id, &Event::Launcher(snapshot(&state, &self.apps.read())));
    }

    pub fn search_apps(&self, query: &str) -> Vec<LauncherItem> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let state = lock(&self.state);
        let apps = self.apps.read();
        let mut found: Vec<(u32, &Entry, Vec<nova_launcher_api::TextRun>)> = apps
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
                    provider: nova_launcher_api::PROVIDER_APPS.to_string(),
                    id: app.id,
                    name,
                    description: app.description,
                    icon: app.icon,
                    file: None,
                }
            })
            .collect()
    }

    pub fn launch(&self, id: &str, focus: impl FnOnce(&[String]) -> Result<bool, String>) -> Result<(), String> {
        let ids = self.window_ids(id).ok_or_else(|| format!("no app {id:?}"))?;
        if self.stopped() {
            return Err(nova_modules::not_ready(FEATURE_ID));
        }
        let focused = focus(&ids).unwrap_or_else(|e| {
            self.logger.warn(LOG_SOURCE, &format!("the compositor can't help with {id}, so starting it: {e}"));
            false
        });
        if !focused {
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

    pub fn start_app(&self, id: &str) -> Result<(), String> {
        let entry = self.apps.entry(id).ok_or_else(|| format!("no app {id:?}"))?;
        if self.stopped() {
            return Err(nova_modules::not_ready(FEATURE_ID));
        }
        nova_apps::launch::launch(&entry, &self.paths.runner, &self.path_var, &self.logger)
            .inspect_err(|e| self.logger.warn(LOG_SOURCE, &format!("can't launch {id}: {e}")))
    }

    pub fn window_ids(&self, id: &str) -> Option<Vec<String>> {
        self.apps.window_ids(id)
    }

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

    fn update(&self, change: impl FnOnce(&mut Saved) -> Result<(), String>) -> Result<(), String> {
        let mut state = lock(&self.state);
        if self.stopped() {
            return Err(nova_modules::not_ready(FEATURE_ID));
        }
        let before = state.saved.clone();
        change(&mut state.saved)?;
        if state.saved == before {
            return Ok(());
        }
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

fn snapshot(state: &State, apps: &nova_apps::State) -> nova_launcher_api::Launcher {
    let installed = apps.entries();
    let mut listed: Vec<App> = installed.values().map(|entry| apps.app_of(entry)).collect();
    listed.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    let is_installed = |id: &&String| installed.contains_key(*id);
    let pinned = state.saved.pinned.iter().filter(is_installed).cloned().collect();
    let mut launched: Vec<(&String, &Launches)> = state
        .saved
        .launches
        .iter()
        .filter(|(id, _)| installed.contains_key(*id) && !state.saved.pinned.contains(id))
        .collect();
    launched.sort_by(|a, b| b.1.last.cmp(&a.1.last).then_with(|| a.0.cmp(b.0)));
    let recent = launched.into_iter().take(MAX_RECENT).map(|(id, _)| id.clone()).collect();
    nova_launcher_api::Launcher { apps: listed, pinned, recent }
}
pub fn focus_open(
    id: &str,
    ids: &[String],
    how: &Run,
    start: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    let Run { compositor, tray, output, underway, logger, .. } = *how;
    if let Some(compositor) = compositor {
        if !ids.is_empty() && compositor.focus_app(ids, output)? {
            return Ok(true);
        }
        if let Some(output) = output {
            compositor.focus_output(output)?;
        }
    }
    let Some(tray) = tray.filter(|tray| tray.in_tray(id)) else { return Ok(false) };
    let activated = tray.activate_in_tray(id).unwrap_or_else(|e| {
        logger.warn(LOG_SOURCE, &e);
        false
    });
    let shown = |wait| {
        compositor.is_none_or(|compositor| focus_shown(AppWindows { id, ids }, compositor, output, wait, logger))
    };
    if activated {
        underway();
        if shown(SHOWN_WINDOW_WAIT) {
            return Ok(true);
        }
    }
    start()?;
    underway();
    shown(STARTED_WINDOW_WAIT);
    Ok(true)
}

#[derive(Debug, Clone, Copy)]
pub struct AppWindows<'a> {
    pub id: &'a str,
    pub ids: &'a [String],
}

// niri focuses a window only with an activation token, which a tray icon, a menu or a notification never hands over:
// https://wayland.app/protocols/xdg-activation-v1
pub fn focus_shown(
    app: AppWindows,
    compositor: &Compositor,
    output: Option<&str>,
    wait: Duration,
    logger: &Logger,
) -> bool {
    let AppWindows { id, ids } = app;
    if ids.is_empty() {
        return true;
    }
    let deadline = std::time::Instant::now() + wait;
    loop {
        match compositor.focus_app(ids, output) {
            Ok(true) => return true,
            Ok(false) if std::time::Instant::now() < deadline => nova_base::backoff::wait_out(TRAY_WINDOW_RETRY),
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
