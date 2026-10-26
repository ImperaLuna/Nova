//! The launcher (feature `launcher`): the apps installed, read from their
//! desktop entries and read again when apps are installed or removed,
//! searching them, starting them, and the user's pinned and recent ones,
//! which survive restarts. Each app's icon is looked up as a file when the
//! apps are read (`crate::icons`), so the UI only loads images.

pub mod autostart;
mod desktop_entry;
mod launch;
mod program;
mod rank;
mod scan;

use crate::compositor::Compositor;
use crate::hub::Hub;
use crate::icons::Icons;
use crate::inotify::{self, Inotify};
use crate::logger::Logger;
use crate::tray::Tray;
use crate::{lock, state_file};
use desktop_entry::{Context, Entry, Locale};
pub use launch::{Runner, open};
use nova_protocol::{APP_ICON_MAX_CHARS, App, AppMatch, Event, SEARCH_APPS_MAX_CHARS};
pub use program::program_name;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

pub const FEATURE_ID: &str = "launcher";
const LOG_SOURCE: &str = "launcher";
/// Installing a package writes many entries; they're read once it pauses.
const SETTLE: Duration = Duration::from_millis(500);
/// Matches a search returns at most.
const MAX_MATCHES: usize = 50;
/// Apps that can be pinned at once.
const MAX_PINNED: usize = 32;
/// Recent apps sent, the latest first.
const MAX_RECENT: usize = 8;
/// Apps whose launches are remembered (for recent and for ranking); the
/// least recently launched are forgotten first.
const MAX_REMEMBERED: usize = 200;
/// How long a launch waits for the window of an app it reopened from the
/// tray, to focus it, and how often it looks.
const TRAY_WINDOW_WAIT: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(3) };
const TRAY_WINDOW_RETRY: Duration = Duration::from_millis(100);

/// Where the module reads and keeps things, and how it starts apps.
#[derive(Clone)]
pub struct Paths {
    /// The XDG data folders apps are found in, in order.
    pub data_dirs: Vec<PathBuf>,
    /// Pinned apps and launch counts.
    pub state_file: PathBuf,
    pub runner: Runner,
    /// Where apps' icons are found.
    pub icons: Arc<Icons>,
}

impl Paths {
    pub fn new(state_dir: &Path, path_var: &str, icons: Arc<Icons>) -> Self {
        Self {
            data_dirs: crate::data_dirs(),
            state_file: state_dir.join("launcher.json"),
            runner: Runner::detect(path_var),
            icons,
        }
    }
}

pub struct Launcher {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    paths: Paths,
    /// Where programs are looked for, and the `PATH` apps get.
    path_var: String,
    locale: Locale,
    /// `XDG_CURRENT_DESKTOP`'s names, for `OnlyShowIn` and `NotShowIn`.
    desktops: Vec<String>,
    /// Set by `stop`; the scanning thread checks it when it wakes.
    stopped: AtomicBool,
    /// Ends the watching thread; `None` until it starts.
    watch: Mutex<Option<inotify::Stopper>>,
    /// Called after each look at the apps, outside the lock (see `on_rescan`).
    rescanned: Mutex<Option<Rescanned>>,
}

type Rescanned = Box<dyn Fn() + Send + Sync>;

struct State {
    apps: BTreeMap<String, Entry>,
    /// The apps' icon files, by app id, for those that have one.
    icons: BTreeMap<String, String>,
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
    /// Loads the saved state, then finds the apps and follows changes to
    /// them on a thread of its own.
    pub fn start(paths: Paths, path_var: &str, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let saved = match std::fs::read_to_string(&paths.state_file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", paths.state_file.display()));
                Saved::default()
            }),
            Err(_) => Saved::default(),
        };
        let desktops = current_desktops();
        let launcher = Arc::new(Self {
            state: Mutex::new(State { apps: BTreeMap::new(), icons: BTreeMap::new(), saved, sent: None }),
            hub,
            logger,
            paths,
            path_var: path_var.to_string(),
            locale: Locale::from_env(),
            desktops,
            stopped: AtomicBool::new(false),
            watch: Mutex::new(None),
            rescanned: Mutex::new(None),
        });
        let worker = Arc::clone(&launcher);
        std::thread::spawn(move || worker.run());
        launcher
    }

    /// Stops following the apps and publishes nothing more.
    pub fn stop(&self) {
        let mut watch = lock(&self.watch);
        {
            let _state = lock(&self.state);
            self.stopped.store(true, Ordering::SeqCst);
        }
        if let Some(watch) = watch.take() {
            watch.stop();
        }
    }

    /// Calls `rescanned` after each look at the apps from now on, for what
    /// depends on them (the tray's apps). It may call back into the launcher.
    pub fn on_rescan(&self, rescanned: impl Fn() + Send + Sync + 'static) {
        *lock(&self.rescanned) = Some(Box::new(rescanned));
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Sends a new subscriber, client `id`, the apps as they are, under the lock that publishing
    /// takes, so a newer state published meanwhile can't arrive before it.
    pub fn send_state(&self, id: crate::hub::ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(id, &Event::Launcher(snapshot(&state)));
    }

    /// The apps matching `query`, best first.
    pub fn search(&self, query: &str) -> Result<Vec<AppMatch>, String> {
        if query.chars().count() > SEARCH_APPS_MAX_CHARS {
            return Err(format!("searches are at most {SEARCH_APPS_MAX_CHARS} characters"));
        }
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let state = lock(&self.state);
        let mut found: Vec<(u32, &Entry, Vec<nova_protocol::TextRun>)> = state
            .apps
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
        Ok(found
            .into_iter()
            .take(MAX_MATCHES)
            .map(|(_, entry, name)| AppMatch { id: entry.id.clone(), name })
            .collect())
    }

    /// Focuses a window the app `id` already has, through `focus` (given the
    /// app ids its windows may have, none for a terminal app; `false` when
    /// none is open), or else starts the app; either counts it as launched.
    pub fn launch(&self, id: &str, focus: impl FnOnce(&[String]) -> Result<bool, String>) -> Result<(), String> {
        let off = || crate::features::not_ready(FEATURE_ID);
        let entry = lock(&self.state).apps.get(id).cloned().ok_or_else(|| format!("no app {id:?}"))?;
        let ids = entry.window_ids();
        if self.stopped() {
            return Err(off());
        }
        let focused = focus(&ids).unwrap_or_else(|e| {
            self.logger.warn(LOG_SOURCE, &format!("the compositor can't help with {id}, so starting it: {e}"));
            false
        });
        if !focused {
            // Focusing may have taken a while.
            if self.stopped() {
                return Err(off());
            }
            launch::launch(&entry, &self.paths.runner, &self.path_var, &self.logger)
                .inspect_err(|e| self.logger.warn(LOG_SOURCE, &format!("can't launch {id}: {e}")))?;
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

    /// The installed app whose windows may have one of `ids` (lower case,
    /// the likeliest first), like a tray icon's process names: the first id
    /// that any app matches decides. Of the apps it matches, one it names
    /// (window class or desktop id) beats one that only runs that program:
    /// `steam` is Steam, not a game shortcut whose Exec is `steam`.
    pub fn find_app(&self, ids: &[String]) -> Option<App> {
        let state = lock(&self.state);
        ids.iter().find_map(|id| {
            let matching: Vec<&Entry> = state.apps.values().filter(|entry| entry.window_ids().contains(id)).collect();
            matching.iter().find(|entry| entry.is_named(id)).or(matching.first()).map(|entry| app(entry, &state.icons))
        })
    }

    /// The app ids the windows of the installed app `id` may have; `None`
    /// when no such app is installed.
    pub fn window_ids(&self, id: &str) -> Option<Vec<String>> {
        lock(&self.state).apps.get(id).map(Entry::window_ids)
    }

    /// The icon file for an app stream (`Command::AppIcon`): the installed
    /// app's it belongs to, by its icon name, program and name (`find_app`,
    /// else loosely by the first word of an app's name: Zen plays as `zen`,
    /// its entry is "Zen Browser" in `zen-beta.desktop`), else the icon
    /// theme's for its icon name or program. Without the launcher (the
    /// feature off), only that last part.
    pub fn app_icon(
        launcher: Option<&Self>,
        icons: &Icons,
        icon_name: &str,
        program: &str,
        name: &str,
    ) -> Result<Option<String>, String> {
        if [icon_name, program, name].iter().any(|value| value.chars().count() > APP_ICON_MAX_CHARS) {
            return Err(format!("names are at most {APP_ICON_MAX_CHARS} characters"));
        }
        let icon_name = Some(icon_name.to_string()).filter(|icon_name| !icon_name.is_empty());
        let program = program_name(program);
        let ids: Vec<String> = [icon_name.as_deref(), program.as_deref(), Some(name)]
            .into_iter()
            .flatten()
            .map(str::to_lowercase)
            .filter(|id| !id.is_empty())
            .collect();
        let found = launcher.and_then(|launcher| {
            launcher.find_app(&ids).or_else(|| {
                let state = lock(&launcher.state);
                ids.iter().find_map(|id| {
                    state.apps.values().find(|entry| entry.first_word_is(id)).map(|entry| app(entry, &state.icons))
                })
            })
        });
        Ok(found
            .and_then(|app| app.icon)
            .or_else(|| [icon_name, program].into_iter().flatten().find_map(|name| icons.find(&name))))
    }

    /// Pins the app `id` at the end, or unpins it.
    pub fn pin(&self, id: &str, pinned: bool) -> Result<(), String> {
        if pinned && !lock(&self.state).apps.contains_key(id) {
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
        let event = snapshot(state);
        if state.sent.as_ref() != Some(&event) {
            self.hub.publish(&Event::Launcher(event.clone()));
            state.sent = Some(event);
        }
    }

    /// Reads the apps, then again whenever they change.
    fn run(self: Arc<Self>) {
        let (changed, changes) = mpsc::channel();
        match Inotify::new(inotify::REPLACEMENTS) {
            Ok(watcher) => self.follow(watcher, changed),
            Err(e) => {
                self.logger
                    .warn(LOG_SOURCE, &format!("can't watch for new apps ({e}); they show after `nova restart`"));
                drop(changed);
            }
        }
        // Watched first, so an app installed meanwhile isn't missed.
        self.rescan();
        while changes.recv().is_ok() {
            while changes.recv_timeout(SETTLE).is_ok() {}
            if self.stopped() {
                return;
            }
            self.rescan();
        }
    }

    /// Watches the app folders on a thread that sends `changed` for each
    /// change that may change the apps.
    fn follow(&self, mut watcher: Inotify, changed: mpsc::Sender<()>) {
        {
            let mut watch = lock(&self.watch);
            if self.stopped() {
                return;
            }
            *watch = Some(watcher.stopper());
        }
        let data_dirs = self.paths.data_dirs.clone();
        let logger = Arc::clone(&self.logger);
        let mut points = watch_all(&mut watcher, &data_dirs, &logger);
        std::thread::spawn(move || {
            loop {
                let found = match watcher.read() {
                    Ok(Some(found)) => found,
                    Ok(None) => return,
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("stopped watching for new apps: {e}"));
                        return;
                    }
                };
                if !found.overflowed && !found.found.iter().any(|change| points.counts(&change.path)) {
                    continue;
                }
                // A folder or link may have come or gone: look again at what to watch.
                points = watch_all(&mut watcher, &data_dirs, &logger);
                if changed.send(()).is_err() {
                    return;
                }
            }
        });
    }

    fn rescan(&self) {
        let context =
            Context { locale: &self.locale, desktops: &self.desktops, path_var: &self.path_var, autostart: false };
        let (apps, problems) = scan::scan(&self.paths.data_dirs, &context);
        // Apps bring their icons, so any answer kept may be out of date.
        let icons = &self.paths.icons;
        icons.forget();
        let found = apps
            .values()
            .filter_map(|entry| Some((entry.id.clone(), icons.entry_icon(entry.icon.as_deref()?)?)))
            .collect();
        let mut state = lock(&self.state);
        if self.stopped() {
            return;
        }
        if state.sent.is_none() || state.apps.len() != apps.len() {
            self.logger.info(LOG_SOURCE, &format!("found {} apps", apps.len()));
        }
        // Each problem once, not on every rescan.
        if state.sent.is_none() {
            for problem in problems {
                self.logger.log(nova_protocol::LogLevel::Debug, LOG_SOURCE, &problem);
            }
        }
        state.apps = apps;
        state.icons = found;
        self.publish(&mut state);
        drop(state);
        if let Some(rescanned) = &*lock(&self.rescanned) {
            rescanned();
        }
    }
}

/// Watches what `scan::watch_points` says for `data_dirs`.
/// `XDG_CURRENT_DESKTOP`'s names, like `niri`, which entries show in or not.
fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

fn watch_all(watcher: &mut Inotify, data_dirs: &[PathBuf], logger: &Logger) -> scan::WatchPoints {
    let points = scan::watch_points(data_dirs);
    // The parents first: a folder that's both gets the trees' wider mask.
    for parent in &points.parents {
        if let Err(e) = watcher.watch_for(parent, inotify::ENTRIES) {
            logger.warn(LOG_SOURCE, &format!("can't watch {} for new apps: {e}", parent.display()));
        }
    }
    for tree in &points.trees {
        if let Err(e) = watcher.watch_tree(tree) {
            logger.warn(LOG_SOURCE, &format!("can't watch {} for new apps: {e}", tree.display()));
        }
    }
    points
}

/// How the UI sees `entry`, whose icon may be among `icons`.
fn app(entry: &Entry, icons: &BTreeMap<String, String>) -> App {
    App {
        id: entry.id.clone(),
        name: entry.name.clone(),
        description: entry.generic_name.clone().or_else(|| entry.comment.clone()),
        icon: icons.get(&entry.id).cloned(),
    }
}

/// The event for `state`: apps by name, and the pinned and recent ones
/// that are installed (recent leaves the pinned ones out).
fn snapshot(state: &State) -> nova_protocol::Launcher {
    let mut apps: Vec<App> = state.apps.values().map(|entry| app(entry, &state.icons)).collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    let installed = |id: &&String| state.apps.contains_key(*id);
    let pinned = state.saved.pinned.iter().filter(installed).cloned().collect();
    // A pinned app already shows among the pins.
    let mut launched: Vec<(&String, &Launches)> = state
        .saved
        .launches
        .iter()
        .filter(|(id, _)| state.apps.contains_key(*id) && !state.saved.pinned.contains(id))
        .collect();
    launched.sort_by(|a, b| b.1.last.cmp(&a.1.last).then_with(|| a.0.cmp(b.0)));
    let recent = launched.into_iter().take(MAX_RECENT).map(|(id, _)| id.clone()).collect();
    nova_protocol::Launcher { apps, pinned, recent }
}
/// The focus step of [`Launcher::launch`] for app `id`, whose windows may
/// have the app ids `ids`: an open window takes focus where it is, unless
/// a `new_window` is asked for; a new one opens on `output` (the
/// launcher's screen), which the compositor puts it on once focused. An
/// app that only sits in the tray shows its window from there, as starting
/// it again may just quit (Discord), unless no window shows: Claude Desktop
/// ignores `Activate` (its icon only has a menu) but shows its window when
/// started again. `true` when nothing needs starting.
pub fn focus_open(
    id: &str,
    ids: &[String],
    new_window: bool,
    compositor: Option<&Compositor>,
    tray: Option<&Tray>,
    output: Option<&str>,
    logger: &Logger,
) -> Result<bool, String> {
    if let Some(compositor) = compositor {
        if !new_window && !ids.is_empty() && compositor.focus_app(ids, output)? {
            return Ok(true);
        }
        if let Some(output) = output {
            compositor.focus_output(output)?;
        }
    }
    let tray = tray.filter(|_| !new_window);
    if !tray.map_or(Ok(false), |tray| tray.activate_app(id))? {
        return Ok(false);
    }
    Ok(compositor.is_none_or(|compositor| focus_shown(id, ids, compositor, output, logger)))
}

/// Focuses the window of app `id` (with one of the app ids `ids`) that its
/// tray icon was just asked to show, once it shows: niri gives it no focus,
/// as a tray click hands the app no activation token. Gives up (logged)
/// after [`TRAY_WINDOW_WAIT`]. `false` when no window showed; `true` also
/// when that can't be told (an app in a terminal, the compositor failing).
pub fn focus_shown(id: &str, ids: &[String], compositor: &Compositor, output: Option<&str>, logger: &Logger) -> bool {
    if ids.is_empty() {
        return true;
    }
    let deadline = std::time::Instant::now() + TRAY_WINDOW_WAIT;
    loop {
        match compositor.focus_app(ids, output) {
            Ok(true) => return true,
            Ok(false) if std::time::Instant::now() < deadline => std::thread::sleep(TRAY_WINDOW_RETRY),
            Ok(false) => {
                logger.info(LOG_SOURCE, &format!("{id}'s window didn't show after its tray icon was asked"));
                return false;
            }
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("can't focus {id}'s window: {e}"));
                return true;
            }
        }
    }
}
