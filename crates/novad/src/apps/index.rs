//! The app index: every installed app's desktop entry, read when it starts
//! following them and again when apps are installed or removed, with each
//! app's icon file looked up as the entries are read (`crate::icons`).
//! novad has one for its whole life; it follows the apps while any module
//! that names them runs (`Module::NEEDS_APPS`), so the tray, notifications
//! and media keep their app names with the launcher off, and forgets them
//! while none does. Whoever depends on the apps hears of each look at them
//! (`on_change`).

use super::desktop_entry::{Context, Entry, Locale};
use super::{program_name, scan};
use crate::icons::Icons;
use crate::inotify::{self, Inotify};
use crate::lock;
use crate::logger::Logger;
use nova_protocol::{APP_ICON_MAX_CHARS, App};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::time::Duration;

const LOG_SOURCE: &str = "apps";
/// Installing a package writes many entries; they're read once it pauses.
const SETTLE: Duration = Duration::from_millis(500);
/// How long a lookup made while novad starts (`app_once_scanned`) waits
/// for the apps to be read the first time.
const FIRST_SCAN_WAIT: Duration = Duration::from_secs(2);
/// The desktop entry category of web browsers.
const BROWSER: &str = "WebBrowser";

/// Where the apps are found.
#[derive(Clone)]
pub struct Sources {
    /// The XDG data folders apps are found in, in order.
    pub data_dirs: Vec<PathBuf>,
    /// Where apps' icons are found.
    pub icons: Arc<Icons>,
}

impl Sources {
    pub fn system(icons: Arc<Icons>) -> Self {
        Self { data_dirs: crate::data_dirs(), icons }
    }
}

pub struct AppIndex {
    state: Mutex<State>,
    /// Wakes lookups waiting for the first look at the apps.
    scanned: Condvar,
    logger: Arc<Logger>,
    sources: Sources,
    /// Where programs are looked for, for `TryExec`.
    path_var: String,
    locale: Locale,
    /// `XDG_CURRENT_DESKTOP`'s names, for `OnlyShowIn` and `NotShowIn`.
    desktops: Vec<String>,
    /// Following the apps (`follow`), until `forget`.
    following: Mutex<Option<Following>>,
    /// Whether it follows them, for lookups waiting on the first read.
    active: AtomicBool,
    /// Called after each look at the apps, outside the lock.
    listeners: Mutex<Vec<Listener>>,
}

/// Told the apps changed; `false` once whoever listened is gone.
type Listener = Box<dyn Fn() -> bool + Send + Sync>;

/// One run of following the apps, from `follow` to `forget`.
struct Following {
    /// Set by `forget`; the scanning thread checks it when it wakes.
    stopped: Arc<AtomicBool>,
    /// Ends the watching thread; `None` until it starts.
    watch: Arc<Mutex<Option<inotify::Stopper>>>,
}

/// The apps as last read.
pub struct State {
    apps: BTreeMap<String, Entry>,
    /// The apps have been read at least once.
    scanned: bool,
    /// The apps' icon files, by app id, for those that have one.
    icons: BTreeMap<String, String>,
}

impl AppIndex {
    /// An index that knows no apps until it follows them.
    pub fn new(sources: Sources, path_var: &str, logger: Arc<Logger>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State { apps: BTreeMap::new(), scanned: false, icons: BTreeMap::new() }),
            scanned: Condvar::new(),
            logger,
            sources,
            path_var: path_var.to_string(),
            locale: Locale::from_env(),
            desktops: super::current_desktops(),
            following: Mutex::new(None),
            active: AtomicBool::new(false),
            listeners: Mutex::new(Vec::new()),
        })
    }

    /// Finds the apps, then follows changes to them on a thread of its own,
    /// unless it already does.
    pub fn follow(self: &Arc<Self>) {
        let mut following = lock(&self.following);
        if following.is_some() {
            return;
        }
        let run = Following { stopped: Arc::new(AtomicBool::new(false)), watch: Arc::new(Mutex::new(None)) };
        let (worker, stopped, watch) = (Arc::clone(self), Arc::clone(&run.stopped), Arc::clone(&run.watch));
        self.active.store(true, Ordering::SeqCst);
        std::thread::spawn(move || worker.run(&stopped, &watch));
        *following = Some(run);
    }

    /// Stops following the apps and forgets them, so they take no memory
    /// while nothing names apps.
    pub fn forget(&self) {
        let Some(run) = lock(&self.following).take() else { return };
        let mut watch = lock(&run.watch);
        {
            let mut state = lock(&self.state);
            run.stopped.store(true, Ordering::SeqCst);
            self.active.store(false, Ordering::SeqCst);
            *state = State { apps: BTreeMap::new(), scanned: false, icons: BTreeMap::new() };
            self.scanned.notify_all();
        }
        if let Some(watch) = watch.take() {
            watch.stop();
        }
    }

    /// Calls `changed` with `listener` after each look at the apps from
    /// now on, for as long as `listener` lives. It may call back into the
    /// index.
    pub fn on_change<T: Send + Sync + 'static>(&self, listener: &Arc<T>, changed: fn(&T)) {
        let listener = Arc::downgrade(listener);
        lock(&self.listeners).push(Box::new(move || {
            let Some(listener) = listener.upgrade() else { return false };
            changed(&listener);
            true
        }));
    }

    /// The apps as they are, for reading several things at once.
    pub fn read(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// The installed app `id` (its desktop file id). Right after the index
    /// starts following the apps, it first waits (a while at most) for the apps to be read, so
    /// an app isn't taken for missing meanwhile.
    pub fn app_once_scanned(&self, id: &str) -> Option<App> {
        let state = lock(&self.state);
        let (state, _) = self
            .scanned
            .wait_timeout_while(state, FIRST_SCAN_WAIT, |state| !state.scanned && self.active.load(Ordering::SeqCst))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.app(id)
    }

    /// The installed app whose windows may have one of `ids` (see
    /// [`State::find_app`]).
    pub fn find_app(&self, ids: &[String]) -> Option<App> {
        lock(&self.state).find_app(ids)
    }

    /// Whether the installed app `id` is a web browser (`WebBrowser` among
    /// its categories).
    pub fn is_browser(&self, id: &str) -> bool {
        lock(&self.state).apps.get(id).is_some_and(|entry| entry.categories.iter().any(|category| category == BROWSER))
    }

    /// The app ids the windows of the installed app `id` may have; `None`
    /// when no such app is installed.
    pub fn window_ids(&self, id: &str) -> Option<Vec<String>> {
        lock(&self.state).apps.get(id).map(Entry::window_ids)
    }

    /// The desktop entry of the installed app `id`.
    pub fn entry(&self, id: &str) -> Option<Entry> {
        lock(&self.state).apps.get(id).cloned()
    }

    /// The icon file for an app stream (`Command::AppIcon`): the installed
    /// app's it belongs to, by its icon name, program and name (`find_app`,
    /// else loosely by the first word of an app's name: Zen plays as `zen`,
    /// its entry is "Zen Browser" in `zen-beta.desktop`), else the icon
    /// theme's for its icon name or program: only that last part while
    /// nothing that names apps is on.
    pub fn app_icon(
        &self,
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
        let found = {
            let state = lock(&self.state);
            state.find_app(&ids).or_else(|| {
                ids.iter().find_map(|id| {
                    state.apps.values().find(|entry| entry.first_word_is(id)).map(|entry| state.app_of(entry))
                })
            })
        };
        Ok(found
            .and_then(|app| app.icon)
            .or_else(|| [icon_name, program].into_iter().flatten().find_map(|name| icons.find(&name))))
    }

    /// Reads the apps, then again whenever they change.
    fn run(&self, stopped: &AtomicBool, watch: &Mutex<Option<inotify::Stopper>>) {
        let (changed, changes) = mpsc::channel();
        match Inotify::new(inotify::REPLACEMENTS) {
            Ok(watcher) => self.watch(watcher, changed, stopped, watch),
            Err(e) => {
                self.logger
                    .warn(LOG_SOURCE, &format!("can't watch for new apps ({e}); they show after `nova restart`"));
                drop(changed);
            }
        }
        // Watched first, so an app installed meanwhile isn't missed.
        self.rescan(stopped);
        while changes.recv().is_ok() {
            while changes.recv_timeout(SETTLE).is_ok() {}
            if stopped.load(Ordering::SeqCst) {
                return;
            }
            self.rescan(stopped);
        }
    }

    /// Watches the app folders on a thread that sends `changed` for each
    /// change that may change the apps.
    fn watch(
        &self,
        mut watcher: Inotify,
        changed: mpsc::Sender<()>,
        stopped: &AtomicBool,
        watch: &Mutex<Option<inotify::Stopper>>,
    ) {
        {
            let mut watch = lock(watch);
            if stopped.load(Ordering::SeqCst) {
                return;
            }
            *watch = Some(watcher.stopper());
        }
        let data_dirs = self.sources.data_dirs.clone();
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

    fn rescan(&self, stopped: &AtomicBool) {
        let context =
            Context { locale: &self.locale, desktops: &self.desktops, path_var: &self.path_var, autostart: false };
        let (apps, problems) = scan::scan(&self.sources.data_dirs, &context);
        // Apps bring their icons, so any answer kept may be out of date.
        let icons = &self.sources.icons;
        icons.forget();
        let found = apps
            .values()
            .filter_map(|entry| Some((entry.id.clone(), icons.entry_icon(entry.icon.as_deref()?)?)))
            .collect();
        let mut state = lock(&self.state);
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        if !state.scanned || state.apps.len() != apps.len() {
            self.logger.info(LOG_SOURCE, &format!("found {} apps", apps.len()));
        }
        // Each problem once, not on every rescan.
        if !state.scanned {
            for problem in problems {
                self.logger.log(nova_protocol::LogLevel::Debug, LOG_SOURCE, &problem);
            }
        }
        state.apps = apps;
        state.icons = found;
        state.scanned = true;
        self.scanned.notify_all();
        drop(state);
        // Taken out first, so a listener may add another meanwhile.
        let listeners = std::mem::take(&mut *lock(&self.listeners));
        let kept: Vec<Listener> = listeners.into_iter().filter(|changed| changed()).collect();
        lock(&self.listeners).splice(0..0, kept);
    }
}

impl State {
    /// Whether the apps have been read at least once.
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    /// Every installed app, by its id.
    pub fn entries(&self) -> &BTreeMap<String, Entry> {
        &self.apps
    }

    /// The installed app `id`, as the UI sees it.
    pub fn app(&self, id: &str) -> Option<App> {
        self.apps.get(id).map(|entry| self.app_of(entry))
    }

    /// How the UI sees `entry`.
    pub fn app_of(&self, entry: &Entry) -> App {
        App {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: entry.generic_name.clone().or_else(|| entry.comment.clone()),
            icon: self.icons.get(&entry.id).cloned(),
        }
    }

    /// The installed app whose windows may have one of `ids` (lower case,
    /// the likeliest first), like a tray icon's process names: the first id
    /// that any app matches decides. Of the apps it matches, one it names
    /// (window class or desktop id) beats one that only runs that program:
    /// `steam` is Steam, not a game shortcut whose Exec is `steam`.
    pub fn find_app(&self, ids: &[String]) -> Option<App> {
        ids.iter().find_map(|id| {
            let matching: Vec<&Entry> = self.apps.values().filter(|entry| entry.window_ids().contains(id)).collect();
            matching.iter().find(|entry| entry.is_named(id)).or(matching.first()).map(|entry| self.app_of(entry))
        })
    }
}

#[cfg(test)]
impl AppIndex {
    /// An index of the apps under `share` (its `applications` folder),
    /// following them; without `share`, one that knows no apps.
    pub fn of(share: Option<&std::path::Path>, logger: &Arc<Logger>) -> Arc<Self> {
        let data_dirs: Vec<PathBuf> = share.into_iter().map(std::path::Path::to_owned).collect();
        let icons = Arc::new(Icons::new(crate::icons::HICOLOR.into(), &data_dirs, None));
        let index = Self::new(Sources { data_dirs, icons }, "", Arc::clone(logger));
        if share.is_some() {
            index.follow();
        }
        index
    }
}

/// Watches what `scan::watch_points` says for `data_dirs`.
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
