use super::desktop_entry::{Context, Entry, Locale};
use super::{program_name, scan};
use crate::APP_ICON_MAX_CHARS;
use nova_app_info::App;
use nova_base::lock;
use nova_icons::Icons;
use nova_inotify::{self as inotify, Inotify};
use nova_log::Logger;
use nova_watch::{self as watch_points, Reporting};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::time::Duration;

const LOG_SOURCE: &str = "apps";
const FIRST_SCAN_WAIT: Duration = Duration::from_secs(2);
const BROWSER: &str = "WebBrowser";

#[derive(Clone)]
pub struct Sources {
    pub data_dirs: Vec<PathBuf>,
    pub icons: Arc<Icons>,
}

impl Sources {
    pub fn system(icons: Arc<Icons>) -> Self {
        Self { data_dirs: nova_paths::data_dirs(), icons }
    }
}

pub struct AppIndex {
    state: Mutex<State>,
    scanned: Condvar,
    logger: Arc<Logger>,
    sources: Sources,
    path_var: String,
    locale: Locale,
    desktops: Vec<String>,
    following: Mutex<Option<Following>>,
    active: AtomicBool,
    listeners: Mutex<Vec<Listener>>,
}

type Listener = Box<dyn Fn(bool) -> bool + Send + Sync>;

struct Following {
    stopped: Arc<AtomicBool>,
    watch: Arc<Mutex<Option<inotify::Stopper>>>,
}

pub struct State {
    apps: BTreeMap<String, Entry>,
    unlisted: BTreeMap<String, Entry>,
    scanned: bool,
    icons: BTreeMap<String, String>,
}

impl AppIndex {
    pub fn new(sources: Sources, path_var: &str, logger: Arc<Logger>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                apps: BTreeMap::new(),
                unlisted: BTreeMap::new(),
                scanned: false,
                icons: BTreeMap::new(),
            }),
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

    pub fn forget(&self) {
        let Some(run) = lock(&self.following).take() else { return };
        let mut watch = lock(&run.watch);
        {
            let mut state = lock(&self.state);
            run.stopped.store(true, Ordering::SeqCst);
            self.active.store(false, Ordering::SeqCst);
            *state = State { apps: BTreeMap::new(), unlisted: BTreeMap::new(), scanned: false, icons: BTreeMap::new() };
            self.scanned.notify_all();
        }
        if let Some(watch) = watch.take() {
            watch.stop();
        }
    }

    pub fn on_change<T: Send + Sync + 'static>(&self, listener: &Arc<T>, changed: fn(&T)) {
        let listener = Arc::downgrade(listener);
        let mut listeners = lock(&self.listeners);
        listeners.retain(|alive| alive(false));
        listeners.push(Box::new(move |tell| {
            let Some(listener) = listener.upgrade() else { return false };
            if tell {
                changed(&listener);
            }
            true
        }));
    }

    pub fn read(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub fn app_once_scanned(&self, id: &str) -> Option<App> {
        let state = lock(&self.state);
        let (state, _) = self
            .scanned
            .wait_timeout_while(state, FIRST_SCAN_WAIT, |state| !state.scanned && self.active.load(Ordering::SeqCst))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.app(id)
    }

    pub fn find_app(&self, ids: &[String]) -> Option<App> {
        lock(&self.state).find_app(ids)
    }

    pub fn window_ids(&self, id: &str) -> Option<Vec<String>> {
        lock(&self.state).apps.get(id).map(Entry::window_ids)
    }

    pub fn entry(&self, id: &str) -> Option<Entry> {
        lock(&self.state).apps.get(id).cloned()
    }

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
        self.rescan(stopped);
        while watch_points::settled(&changes) {
            if stopped.load(Ordering::SeqCst) {
                return;
            }
            self.rescan(stopped);
        }
    }

    fn watch(
        &self,
        watcher: Inotify,
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
        let reporting = Reporting { logger: Arc::clone(&self.logger), source: LOG_SOURCE, what: "new apps" };
        watch_points::follow(watcher, move || scan::watch_points(&data_dirs), changed, reporting);
    }

    fn rescan(&self, stopped: &AtomicBool) {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        let context =
            Context { locale: &self.locale, desktops: &self.desktops, path_var: &self.path_var, autostart: false };
        let scan::Found { apps, unlisted, problems } = scan::scan(&self.sources.data_dirs, &context);
        let icons = &self.sources.icons;
        icons.forget();
        let found = apps
            .values()
            .chain(unlisted.values())
            .filter_map(|entry| Some((entry.id.clone(), icons.entry_icon(entry.icon.as_deref()?)?)))
            .collect();
        let mut state = lock(&self.state);
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        if !state.scanned || state.apps.len() != apps.len() {
            self.logger.info(LOG_SOURCE, &format!("found {} apps", apps.len()));
        }
        if !state.scanned {
            for problem in problems {
                self.logger.log(nova_carrier::log::LogLevel::Debug, LOG_SOURCE, &problem);
            }
        }
        state.apps = apps;
        state.unlisted = unlisted;
        state.icons = found;
        state.scanned = true;
        self.scanned.notify_all();
        drop(state);
        let listeners = std::mem::take(&mut *lock(&self.listeners));
        let kept: Vec<Listener> = listeners.into_iter().filter(|changed| changed(true)).collect();
        lock(&self.listeners).splice(0..0, kept);
    }
}

impl State {
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    pub fn is_browser(&self, id: &str) -> bool {
        self.apps.get(id).is_some_and(|entry| entry.categories.iter().any(|category| category == BROWSER))
    }

    pub fn entries(&self) -> &BTreeMap<String, Entry> {
        &self.apps
    }

    pub fn app(&self, id: &str) -> Option<App> {
        self.apps.get(id).map(|entry| self.app_of(entry))
    }

    pub fn app_of(&self, entry: &Entry) -> App {
        App {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: entry.generic_name.clone().or_else(|| entry.comment.clone()),
            icon: self.icons.get(&entry.id).cloned(),
        }
    }

    pub fn find_app(&self, ids: &[String]) -> Option<App> {
        ids.iter().find_map(|id| {
            let matching: Vec<&Entry> = self.apps.values().filter(|entry| entry.window_ids().contains(id)).collect();
            matching.iter().find(|entry| entry.is_named(id)).or(matching.first()).map(|entry| self.app_of(entry))
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
impl AppIndex {
    pub fn of(share: Option<&std::path::Path>, logger: &Arc<Logger>) -> Arc<Self> {
        let data_dirs: Vec<PathBuf> = share.into_iter().map(std::path::Path::to_owned).collect();
        let icons = Arc::new(Icons::new(nova_icons::HICOLOR.into(), &data_dirs, None));
        let index = Self::new(Sources { data_dirs, icons }, "", Arc::clone(logger));
        if share.is_some() {
            index.follow();
        }
        index
    }
}
