//! Wallpapers (feature `wallpaper`, and `wallpaper-engine` for dynamic
//! ones): one on every output, or one of its own on an output, set in
//! config.toml's `[wallpaper]` or with `nova wallpaper`, which wins until
//! that section changes. A wallpaper is an image, a video, a Wallpaper
//! Engine project folder or a Steam Workshop ID (`source`).
//!
//! awww draws still images: novad runs its daemon in a namespace of Nova's
//! own (so a copy the user runs never clashes with it), without its cache,
//! so what it shows only ever comes from novad. A new image fades in on the
//! outputs that change; one fade covers every output that gets the same
//! image. An output that's plugged in (or comes back) gets its image as
//! soon as the compositor reports it.
//!
//! Dynamic wallpapers are drawn by one linux-wallpaperengine (`engine`),
//! started again whenever what it shows changes. awww shows a still of it
//! underneath, so it fades in first and shows while the engine (re)starts:
//! the frame the engine last drew on an output that size, kept from each
//! run, or the project's own preview (a small thumbnail) until there's one.
//! The engine starts after awww does, and again whenever awww starts again,
//! so it stays above awww's surfaces.
//!
//! Each program is started again when it exits on its own, waiting longer
//! each time it keeps failing, and ended when it has nothing to show, so a
//! wallpaper tool of the user's own keeps working while nothing is set.
//!
//! One thread owns both programs and sends the images; everything else
//! changes what it wants under the lock and wakes it.
//!
//! `folder` lists the wallpaper folder for the carousel.

mod config;
mod engine;
pub mod folder;
mod source;

pub use config::WallpaperConfig;
use nova_base::backoff::{Backoff, Restarts};
use nova_base::{lock, state_file};
use nova_carrier::log::LogLevel;
use nova_inotify as inotify;
use nova_log::Logger;
use nova_modules::not_ready;
use nova_process::child;
use nova_wallpaper_api::Wallpapers;
use serde::{Deserialize, Serialize};
use source::{Kind, Source};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "wallpaper";
/// The feature that adds dynamic wallpapers.
pub const ENGINE_FEATURE_ID: &str = "wallpaper-engine";
const LOG_SOURCE: &str = "wallpaper";
const DAEMON: &str = "awww-daemon";
const CLIENT: &str = "awww";
/// The awww namespace of Nova's daemon.
const NAMESPACE: &str = "nova";
/// How a new image arrives (the user's pick from the IMP-168 mockup): a
/// one-second fade, drawn at a rate that stays smooth on high refresh
/// screens (awww's default is 30 fps).
const TRANSITION: &str = "fade";
const TRANSITION_SECONDS: &str = "1";
const TRANSITION_FPS: &str = "120";
/// Images fill the output and are cropped to fit, never stretched.
const RESIZE: &str = "crop";
/// What `nova wallpaper` chose, in the state folder.
const STATE_FILE: &str = "wallpaper.json";
/// How long a started daemon has to start answering.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL: Duration = Duration::from_millis(50);
/// How long `awww query` may take while the daemon starts.
const QUERY_TIMEOUT: Duration = Duration::from_secs(1);
/// How long `awww img` may take: it decodes and scales the image itself.
const IMAGE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a started engine's frame is waited for: it writes it once it
/// drew a few frames, which it doesn't while paused under a fullscreen
/// window.
const FRAME_WAIT: Duration = Duration::from_secs(30);
/// Waits before sending an image again after `awww img` failed: doubling
/// from the first while it keeps failing.
const FIRST_RESEND: Duration = Duration::from_secs(1);
const MAX_RESEND: Duration = Duration::from_secs(60);

/// The wallpaper for every output and the outputs with one of their own:
/// as the user named them ([`Source`]s), or as a program shows them (paths).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T: Deserialize<'de>"))]
struct Choice<T = Source> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<T>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    outputs: BTreeMap<String, T>,
}

impl<T> Default for Choice<T> {
    fn default() -> Self {
        Self { path: None, outputs: BTreeMap::new() }
    }
}

impl<T: Clone> Choice<T> {
    fn is_empty(&self) -> bool {
        self.path.is_none() && self.outputs.is_empty()
    }

    /// The wallpaper each of `names` shows; one with none is left out.
    fn per_output(&self, names: &[String]) -> BTreeMap<String, T> {
        names
            .iter()
            .filter_map(|name| Some((name.clone(), self.outputs.get(name).or(self.path.as_ref())?.clone())))
            .collect()
    }

    /// Every wallpaper in it.
    fn all(&self) -> impl Iterator<Item = &T> {
        self.path.iter().chain(self.outputs.values())
    }
}

impl Choice {
    /// The choice from config.toml's `[wallpaper]`, leaving out (and logging)
    /// settings that aren't absolute paths or Workshop IDs.
    fn from_config(config: &WallpaperConfig, logger: &Logger) -> Self {
        let resolve = |setting: String, text: &str| {
            Source::from_config(&setting, text).map_err(|e| logger.warn(LOG_SOURCE, &format!("{e}, ignoring it"))).ok()
        };
        Self {
            path: config.path.as_deref().and_then(|path| resolve("[wallpaper] path".into(), path)),
            outputs: config
                .outputs
                .iter()
                .filter_map(|(name, path)| Some((name.clone(), resolve(format!("[wallpaper.outputs] {name}"), path)?)))
                .collect(),
        }
    }

    fn wire(&self) -> Wallpapers {
        Wallpapers {
            path: self.path.as_ref().map(Source::to_string),
            outputs: self.outputs.iter().map(|(name, source)| (name.clone(), source.to_string())).collect(),
        }
    }
}

/// The state file: `nova wallpaper`'s choice, and the `[wallpaper]` config
/// it was made over. A config that differs since means the user edited it,
/// which wins.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    config: WallpaperConfig,
    choice: Choice,
}

/// The outputs to show wallpapers on.
#[derive(Debug, Clone, PartialEq)]
enum Outputs {
    /// As the compositor last reported them.
    Known(Vec<String>),
    /// The compositor hasn't reported them yet: nothing is shown until it
    /// does, so every output gets its own wallpaper in one go.
    Unknown,
    /// novad doesn't follow this compositor: images go to whatever outputs
    /// awww sees, and only wallpapers set for an output by name can be
    /// dynamic.
    Unfollowed,
}

/// Where the thread finds and keeps things.
struct Places {
    /// PATH for the programs it runs.
    path_var: String,
    /// The home folder, where Steam libraries are looked for.
    home: Option<PathBuf>,
    /// `<state>/wallpaper-engine`.
    engine: PathBuf,
}

impl Places {
    fn steam_roots(&self) -> Vec<PathBuf> {
        source::steam_roots(self.home.as_deref())
    }
}

pub struct Wallpaper {
    shared: Arc<Shared>,
    places: Arc<Places>,
    state_file: PathBuf,
    logger: Arc<Logger>,
}

struct Shared {
    state: Mutex<State>,
    /// Wakes the thread on any change to `state`.
    wake: Condvar,
}

struct State {
    /// Which features are ready: still images (`wallpaper`) and dynamic
    /// wallpapers (`wallpaper-engine`).
    images_ready: bool,
    engine_ready: bool,
    /// The last `[wallpaper]` seen, `None` before the first.
    config: Option<WallpaperConfig>,
    from_config: Choice,
    /// `nova wallpaper`'s choice, over the config's.
    chosen: Option<Choice>,
    outputs: Outputs,
    /// Outputs whose wallpaper is sent again even when it's the same:
    /// unplugged since the thread last looked (awww forgot their image,
    /// even when they're back by then) or chosen again with `nova
    /// wallpaper` (the file may have changed). `all_stale`: every output.
    stale: BTreeSet<String>,
    all_stale: bool,
    /// awww's daemon and linux-wallpaperengine.
    daemon: Running,
    engine: Running,
    /// What's wanted changed since the thread last looked.
    changed: bool,
    /// A file was written in the engine's folder (its frame, say) since
    /// the thread last looked, and what watches it.
    frame_written: bool,
    frame_watch: Option<inotify::Stopper>,
    stopped: bool,
}

impl State {
    fn current(&self) -> &Choice {
        self.chosen.as_ref().unwrap_or(&self.from_config)
    }
}

/// A program the thread keeps running.
#[derive(Default)]
struct Running {
    /// Its pid, for `stop` and to end it when it isn't wanted. Forgotten
    /// before it's reaped.
    pid: Option<u32>,
    /// novad asked it to end, so its exit isn't a crash.
    ending: bool,
    /// It exited without being asked, since the thread last looked.
    crashed: bool,
}

impl Running {
    /// Asks it to end. Under the state lock: the pid is forgotten under it
    /// before the reap, so it's still the program's (at worst a zombie).
    fn end(&mut self) {
        if let Some(pid) = self.pid {
            self.ending = true;
            child::terminate(pid);
        }
    }
}

impl Wallpaper {
    /// Starts its thread, which runs nothing until
    /// [`configure`](Self::configure) says a feature is ready and a
    /// wallpaper is set. `follows_compositor`: [`outputs`](Self::outputs)
    /// will report the outputs.
    pub fn start(
        state_dir: &Path,
        home: Option<PathBuf>,
        path_var: String,
        follows_compositor: bool,
        logger: Arc<Logger>,
    ) -> Arc<Self> {
        let state = State {
            images_ready: false,
            engine_ready: false,
            config: None,
            from_config: Choice::default(),
            chosen: None,
            outputs: if follows_compositor { Outputs::Unknown } else { Outputs::Unfollowed },
            stale: BTreeSet::new(),
            all_stale: false,
            daemon: Running::default(),
            engine: Running::default(),
            changed: false,
            frame_written: false,
            frame_watch: None,
            stopped: false,
        };
        let shared = Arc::new(Shared { state: Mutex::new(state), wake: Condvar::new() });
        let places = Arc::new(Places { path_var, home, engine: engine::state_folder(state_dir) });
        let running = Arc::clone(&shared);
        let (thread_places, thread_logger) = (Arc::clone(&places), Arc::clone(&logger));
        std::thread::spawn(move || running.run(&thread_places, &thread_logger));
        Arc::new(Self { shared, places, state_file: state_dir.join(STATE_FILE), logger })
    }

    /// Which features are ready (still images, dynamic wallpapers), and
    /// config.toml's `[wallpaper]`. Called at start and on every reload. A
    /// changed section drops what `nova wallpaper` chose; at start, so does
    /// one changed while novad wasn't running. The section's `folder` doesn't
    /// count: it only lists images to pick from.
    pub fn configure(&self, images_ready: bool, engine_ready: bool, config: &WallpaperConfig) {
        let config = &config.images();
        let mut state = lock(&self.shared.state);
        if state.config.as_ref() != Some(config) {
            let first = state.config.is_none();
            state.from_config = Choice::from_config(config, &self.logger);
            state.chosen = match first {
                true => self.saved_over(config),
                false => None,
            };
            if state.chosen.is_none() {
                self.forget_saved();
            }
            state.config = Some(config.clone());
        }
        state.images_ready = images_ready;
        state.engine_ready = engine_ready;
        self.changed(state);
    }

    /// The compositor's outputs, whenever they change.
    pub fn outputs(&self, names: Vec<String>) {
        let mut state = lock(&self.shared.state);
        if let Outputs::Known(before) = &state.outputs {
            let gone: Vec<String> = before.iter().filter(|name| !names.contains(name)).cloned().collect();
            state.stale.extend(gone);
        }
        let outputs = Outputs::Known(names);
        if state.outputs != outputs {
            state.outputs = outputs;
            self.changed(state);
        }
    }

    /// Shows `path` (an absolute path, or a Workshop ID) on every output, or
    /// only on `output`, and remembers it (`nova wallpaper`).
    pub fn set(&self, path: &str, output: Option<&str>) -> Result<(), String> {
        self.current()?;
        let source = Source::from_request(path)?;
        let kind = source.kind(&self.places.steam_roots())?;
        let mut state = lock(&self.shared.state);
        match kind.is_dynamic() {
            true if !state.engine_ready => return Err(engine_not_ready()),
            false if !state.images_ready => return Err(not_ready(FEATURE_ID)),
            _ => {}
        }
        if kind.is_dynamic() && output.is_none() && state.outputs == Outputs::Unfollowed {
            return Err(
                "novad doesn't follow this compositor's outputs, so set a dynamic wallpaper with --output".into()
            );
        }
        let mut choice = state.current().clone();
        match output {
            Some(name) => {
                check_output(name, &state.outputs)?;
                choice.outputs.insert(name.to_owned(), source);
            }
            None => choice = Choice { path: Some(source), outputs: BTreeMap::new() },
        }
        // Sent again even when it's the same: the file may have changed.
        let stale = output.map(str::to_owned);
        let saved = Saved { config: state.config.clone().unwrap_or_default(), choice };
        state_file::save(&self.state_file, &saved)
            .map_err(|e| format!("can't save the choice to {}: {e}", self.state_file.display()))?;
        state.chosen = Some(saved.choice);
        match stale {
            Some(name) => _ = state.stale.insert(name),
            None => state.all_stale = true,
        }
        self.changed(state);
        Ok(())
    }

    /// What shows now, or would once the programs run.
    pub fn current(&self) -> Result<Wallpapers, String> {
        let state = lock(&self.shared.state);
        match state.images_ready || state.engine_ready {
            true => Ok(state.current().wire()),
            false => Err(not_ready(FEATURE_ID)),
        }
    }

    /// Ends the programs and the thread, for a novad that ends without
    /// exiting the process (novad's `InProcess`).
    pub fn stop(&self) {
        let mut state = lock(&self.shared.state);
        state.stopped = true;
        state.daemon.end();
        state.engine.end();
        if let Some(watch) = state.frame_watch.take() {
            watch.stop();
        }
        self.changed(state);
    }

    fn changed(&self, mut state: MutexGuard<'_, State>) {
        state.changed = true;
        self.shared.wake.notify_all();
    }

    /// The choice saved over `config`, if there's one.
    fn saved_over(&self, config: &WallpaperConfig) -> Option<Choice> {
        let text = match std::fs::read_to_string(&self.state_file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                self.logger.warn(LOG_SOURCE, &format!("can't read {}: {e}", self.state_file.display()));
                return None;
            }
        };
        match serde_json::from_str::<Saved>(&text) {
            Ok(saved) if saved.config == *config => Some(saved.choice),
            Ok(_) => {
                self.logger.info(LOG_SOURCE, "[wallpaper] changed since `nova wallpaper` was used, so it wins");
                None
            }
            Err(e) => {
                self.logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", self.state_file.display()));
                None
            }
        }
    }

    fn forget_saved(&self) {
        match std::fs::remove_file(&self.state_file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                self.logger.warn(LOG_SOURCE, &format!("can't remove {}: {e}", self.state_file.display()));
            }
            _ => {}
        }
    }
}

fn engine_not_ready() -> String {
    format!(
        "dynamic wallpapers need the {ENGINE_FEATURE_ID} feature, which isn't ready: it's off unless features.toml's \
         `enabled` lists it (see `nova status`)"
    )
}

/// An error unless `name` can name an output: awww takes a comma-separated
/// list, and one the compositor reported, when it reports them.
fn check_output(name: &str, outputs: &Outputs) -> Result<(), String> {
    if name.is_empty() || name.contains(',') {
        return Err(format!("{name:?} isn't an output name"));
    }
    match outputs {
        Outputs::Known(names) if !names.iter().any(|known| known == name) => {
            Err(format!("there's no output {name:?} (there's {})", names.join(", ")))
        }
        _ => Ok(()),
    }
}

/// What awww shows on an output.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Picture {
    Image(PathBuf),
    /// A dynamic wallpaper's still: its project's frame kept for the
    /// output's size, else its preview, else none. awww runs under it
    /// even with none, so the outputs' sizes are known to cut its frame.
    Dynamic {
        project: PathBuf,
        preview: Option<PathBuf>,
    },
}

impl Picture {
    /// The image for an output `size` pixels big, when that's known.
    fn image(&self, size: Option<(u32, u32)>, folder: &Path) -> Option<PathBuf> {
        match self {
            Self::Image(path) => Some(path.clone()),
            Self::Dynamic { project, preview } => {
                size.and_then(|size| engine::still(project, size, folder)).or_else(|| preview.clone())
            }
        }
    }
}

/// What to show, worked out from the choice.
#[derive(Default)]
struct Plan {
    /// awww's pictures: still images, and dynamic wallpapers' stills.
    images: Choice<Picture>,
    /// linux-wallpaperengine's project for each output.
    screens: BTreeMap<String, PathBuf>,
}

impl Plan {
    /// Looks up what each wallpaper in `choice` is (logging why one can't be
    /// shown), and makes the projects for videos. Checks `stopped` before
    /// each change to the state folder.
    fn new(
        choice: &Choice,
        outputs: &Outputs,
        (images_ready, engine_ready): (bool, bool),
        places: &Places,
        logger: &Logger,
        stopped: &dyn Fn() -> bool,
    ) -> Self {
        let steam_roots = places.steam_roots();
        let mut kinds: BTreeMap<&Source, Option<Kind>> = BTreeMap::new();
        for source in choice.all() {
            kinds.entry(source).or_insert_with(|| match source.kind(&steam_roots) {
                Ok(kind) if kind.is_dynamic() && !engine_ready => {
                    logger.warn(LOG_SOURCE, &format!("{source} can't move: {}", engine_not_ready()));
                    Some(kind)
                }
                Ok(kind) => Some(kind),
                Err(e) => {
                    logger.warn(LOG_SOURCE, &format!("can't show {source}: {e}"));
                    None
                }
            });
        }
        let mut projects: BTreeMap<&Source, PathBuf> = BTreeMap::new();
        if engine_ready {
            for (source, kind) in &kinds {
                let project = match kind {
                    Some(Kind::Project { folder, .. }) => Some(folder.clone()),
                    Some(Kind::Video(video)) if !stopped() => engine::video_project(video, &places.engine)
                        .map_err(|e| logger.warn(LOG_SOURCE, &format!("can't show {source}: {e}")))
                        .ok(),
                    _ => None,
                };
                if let Some(project) = project {
                    projects.insert(source, project);
                }
            }
            let videos = kinds
                .iter()
                .filter(|(_, kind)| matches!(kind, Some(Kind::Video(_))))
                .filter_map(|(source, _)| projects.get(source).cloned())
                .collect();
            let used: Vec<&Path> = projects.values().map(PathBuf::as_path).collect();
            if !stopped()
                && let Err(e) = engine::forget_videos(&places.engine, &videos)
            {
                logger.warn(LOG_SOURCE, &e);
            }
            // Kept while a wallpaper can't be looked up (a library not
            // mounted yet, say): it may be one of them.
            let dynamic = kinds.values().filter(|kind| kind.as_ref().is_none_or(Kind::is_dynamic)).count();
            if used.len() == dynamic
                && !stopped()
                && let Err(e) = engine::forget_stills(&places.engine, &used)
            {
                logger.warn(LOG_SOURCE, &e);
            }
        }
        let image = |source: &Source| match kinds.get(source)?.as_ref()? {
            _ if !images_ready => None,
            Kind::Image(path) => Some(Picture::Image(path.clone())),
            Kind::Project { folder, preview } => {
                Some(Picture::Dynamic { project: folder.clone(), preview: preview.clone() })
            }
            Kind::Video(_) => Some(Picture::Dynamic { project: projects.get(source)?.clone(), preview: None }),
        };
        let mut plan = Self::default();
        match outputs {
            Outputs::Unknown => {}
            Outputs::Known(names) => {
                for (name, source) in choice.per_output(names) {
                    if let Some(path) = image(&source) {
                        plan.images.outputs.insert(name.clone(), path);
                    }
                    if let Some(project) = projects.get(&source) {
                        plan.screens.insert(name, project.clone());
                    }
                }
            }
            Outputs::Unfollowed => {
                plan.images.path = choice.path.as_ref().and_then(image);
                plan.images.outputs =
                    choice.outputs.iter().filter_map(|(name, source)| Some((name.clone(), image(source)?))).collect();
                plan.screens = choice
                    .outputs
                    .iter()
                    .filter_map(|(name, source)| Some((name.clone(), projects.get(source)?.clone())))
                    .collect();
                if let Some(every) = choice.path.as_ref().filter(|every| projects.contains_key(every)) {
                    let why = "novad doesn't follow this compositor's outputs, so set it for each output by name";
                    logger.warn(LOG_SOURCE, &format!("can't show {every} on every output: {why}"));
                }
            }
        }
        plan
    }
}

/// What awww shows, as far as novad knows; forgotten when the daemon exits.
#[derive(Default)]
struct Shown {
    /// By output, when the outputs are known.
    outputs: BTreeMap<String, PathBuf>,
    /// The whole choice, when they aren't.
    unfollowed: Option<Choice<PathBuf>>,
}

/// What the thread keeps about awww.
struct Awww {
    shown: Shown,
    restarts: Restarts,
    /// When to start it again after it exited on its own.
    start_at: Option<Instant>,
    /// After an `awww img` failed (an output awww hasn't bound yet, say):
    /// when to send it again.
    resend: Backoff,
    resend_at: Option<Instant>,
    /// How often it was started: linux-wallpaperengine starts again after
    /// each, to stay above its surfaces.
    starts: u64,
    /// Each output's size in pixels, as it last said.
    sizes: BTreeMap<String, (u32, u32)>,
}

/// What the thread keeps about linux-wallpaperengine.
#[derive(Default)]
struct EngineRun {
    /// What the running one shows; `None` has it started again.
    shown: Option<BTreeMap<String, PathBuf>>,
    restarts: Restarts,
    start_at: Option<Instant>,
    /// awww's `starts` when it started.
    above: u64,
    /// Its frame, until it's been cut into stills or given up on.
    frame: Option<Frame>,
}

/// A started engine's frame, being waited for.
struct Frame {
    screens: BTreeMap<String, PathBuf>,
    until: Instant,
}

impl Shared {
    fn run(self: &Arc<Self>, places: &Places, logger: &Arc<Logger>) {
        let mut awww = Awww {
            shown: Shown::default(),
            restarts: Restarts::default(),
            start_at: None,
            resend: Backoff::new(FIRST_RESEND, MAX_RESEND),
            resend_at: None,
            starts: 0,
            sizes: BTreeMap::new(),
        };
        let mut engine = EngineRun::default();
        loop {
            let mut state = lock(&self.state);
            let idle = |state: &mut State| {
                !state.stopped
                    && !state.changed
                    && !state.daemon.crashed
                    && !state.engine.crashed
                    && !state.frame_written
            };
            let timers = [awww.start_at, awww.resend_at, engine.start_at];
            let given_up_at = engine.frame.as_ref().map(|frame| frame.until);
            state = match timers.into_iter().chain([given_up_at]).flatten().min() {
                None => self.wake.wait_while(state, idle).unwrap_or_else(|e| e.into_inner()),
                Some(due) => {
                    let wait = due.saturating_duration_since(Instant::now());
                    self.wake.wait_timeout_while(state, wait, idle).unwrap_or_else(|e| e.into_inner()).0
                }
            };
            if state.stopped {
                return;
            }
            // Only the frame was written or waited for long enough: nothing
            // else to work out.
            let now = Instant::now();
            let due = timers.into_iter().flatten().any(|at| at <= now);
            if !state.changed && !state.daemon.crashed && !state.engine.crashed && !due {
                let written = std::mem::take(&mut state.frame_written);
                drop(state);
                let stopped = || lock(&self.state).stopped;
                keep_stills(&mut engine, written, &awww.sizes, places, logger, &stopped);
                continue;
            }
            if std::mem::take(&mut state.daemon.crashed) {
                awww.shown = Shown::default();
                let wait = awww.restarts.after_exit();
                logger.warn(LOG_SOURCE, &format!("{DAEMON} ended, starting it again in {}s", wait.as_secs()));
                awww.start_at = Some(Instant::now() + wait);
            }
            if std::mem::take(&mut state.engine.crashed) {
                engine.shown = None;
                let wait = engine.restarts.after_exit();
                logger
                    .warn(LOG_SOURCE, &format!("{} ended, starting it again in {}s", engine::PROGRAM, wait.as_secs()));
                engine.start_at = Some(Instant::now() + wait);
            }
            if std::mem::take(&mut state.all_stale) {
                awww.shown = Shown::default();
                engine.shown = None;
            }
            for name in std::mem::take(&mut state.stale) {
                awww.shown.outputs.remove(&name);
                awww.shown.unfollowed = None;
                if engine.shown.as_ref().is_some_and(|screens| screens.contains_key(&name)) {
                    engine.shown = None;
                }
            }
            state.changed = false;
            let (images_ready, engine_ready) = (state.images_ready, state.engine_ready);
            let choice = if images_ready || engine_ready { state.current().clone() } else { Choice::default() };
            let outputs = state.outputs.clone();
            drop(state);
            let stopped = || lock(&self.state).stopped;
            let plan = Plan::new(&choice, &outputs, (images_ready, engine_ready), places, logger, &stopped);
            self.drive_awww(&plan.images, &outputs, &mut awww, places, logger);
            self.drive_engine(&plan.screens, &mut engine, awww.starts, places, logger);
        }
    }

    /// Runs awww's daemon while it has images to show, and sends the ones
    /// it doesn't show yet.
    fn drive_awww(
        self: &Arc<Self>,
        images: &Choice<Picture>,
        outputs: &Outputs,
        awww: &mut Awww,
        places: &Places,
        logger: &Arc<Logger>,
    ) {
        let mut state = lock(&self.state);
        if state.stopped {
            return;
        }
        if images.is_empty() {
            if state.daemon.pid.is_some() {
                logger.info(LOG_SOURCE, &format!("no image to show, stopping {DAEMON}"));
                state.daemon.end();
            }
            (awww.start_at, awww.resend_at) = (None, None);
            return;
        }
        // The outputs whose size picks their still. Only ones the compositor
        // reported: one named in the config may not be plugged in.
        let dynamic: Vec<&String> = match outputs {
            Outputs::Known(_) => images
                .outputs
                .iter()
                .filter(|(_, picture)| matches!(picture, Picture::Dynamic { .. }))
                .map(|(name, _)| name)
                .collect(),
            _ => Vec::new(),
        };
        if state.daemon.pid.is_none() {
            if awww.start_at.is_some_and(|at| at > Instant::now()) {
                // Nothing to resend to until it runs again.
                awww.resend_at = None;
                return;
            }
            awww.start_at = None;
            // A new daemon shows nothing yet: it runs without its cache.
            awww.shown = Shown::default();
            awww.restarts.started();
            let mut command = Command::new(DAEMON);
            command.args(["--namespace", NAMESPACE, "--no-cache", "--quiet"]).env("PATH", &places.path_var);
            // Spawned under the lock, so `stop` sees the pid or stops the
            // thread before the spawn.
            if let Err(e) = self.spawn(&mut state, command, |state| &mut state.daemon, DAEMON, logger) {
                logger.warn(LOG_SOURCE, &format!("can't start {DAEMON}: {e}"));
                awww.start_at = Some(Instant::now() + awww.restarts.after_exit());
                return;
            }
            awww.starts += 1;
            drop(state);
            match self.wait_ready(&places.path_var, &dynamic) {
                Ok(sizes) => awww.sizes = sizes,
                Err(e) => {
                    logger.warn(LOG_SOURCE, &format!("{DAEMON} didn't start answering: {e}"));
                    // Not asked to end: it's started again, after a wait,
                    // once it's gone.
                    if let Some(pid) = lock(&self.state).daemon.pid {
                        child::terminate(pid);
                    }
                    return;
                }
            }
        } else {
            drop(state);
            // A mode change since doesn't tell novad.
            if !dynamic.is_empty()
                && let Some(sizes) = query_sizes(&places.path_var)
            {
                awww.sizes = sizes;
            }
        }
        let images = Choice {
            path: images.path.as_ref().and_then(|picture| picture.image(None, &places.engine)),
            outputs: images
                .outputs
                .iter()
                .filter_map(|(name, picture)| {
                    Some((name.clone(), picture.image(awww.sizes.get(name).copied(), &places.engine)?))
                })
                .collect(),
        };
        let stopped = || lock(&self.state).stopped;
        if show(&images, outputs, &mut awww.shown, &places.path_var, logger, &stopped) {
            awww.resend.reset();
            awww.resend_at = None;
        } else {
            awww.resend_at = Some(Instant::now() + awww.resend.next());
        }
    }

    /// Runs linux-wallpaperengine while there are dynamic wallpapers to
    /// show, starting it again when they change or awww started again.
    fn drive_engine(
        self: &Arc<Self>,
        screens: &BTreeMap<String, PathBuf>,
        engine: &mut EngineRun,
        awww_starts: u64,
        places: &Places,
        logger: &Arc<Logger>,
    ) {
        let mut state = lock(&self.state);
        if state.stopped {
            return;
        }
        if screens.is_empty() {
            if state.engine.pid.is_some() {
                logger.info(LOG_SOURCE, &format!("no dynamic wallpaper to show, stopping {}", engine::PROGRAM));
                state.engine.end();
            }
            (engine.shown, engine.start_at, engine.frame) = (None, None, None);
            return;
        }
        if state.engine.pid.is_some() {
            if engine.shown.as_ref() != Some(screens) || engine.above != awww_starts {
                // Started again once it's gone: its exit wakes the thread.
                state.engine.end();
            }
            return;
        }
        if engine.start_at.is_some_and(|at| at > Instant::now()) {
            return;
        }
        engine.start_at = None;
        engine.restarts.started();
        // An older run's frame isn't this one's. The engine doesn't make
        // the folder it writes it to.
        let frame = engine::frame_file(&places.engine);
        let _ = std::fs::remove_file(&frame);
        match std::fs::create_dir_all(&places.engine) {
            Ok(()) if state.frame_watch.is_none() => match self.watch_frames(&places.engine) {
                Ok(watch) => state.frame_watch = Some(watch),
                Err(e) => logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", places.engine.display())),
            },
            Ok(()) => {}
            Err(e) => logger.warn(LOG_SOURCE, &format!("can't make {}: {e}", places.engine.display())),
        }
        let started = engine::assets(&places.steam_roots(), &places.engine).and_then(|assets| {
            let command = engine::command(screens, &assets, &frame, &places.path_var);
            self.spawn(&mut state, command, |state| &mut state.engine, engine::PROGRAM, logger)
                .map_err(|e| e.to_string())
        });
        match started {
            Ok(()) => {
                (engine.shown, engine.above) = (Some(screens.clone()), awww_starts);
                engine.frame = Some(Frame { screens: screens.clone(), until: Instant::now() + FRAME_WAIT });
            }
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("can't start {}: {e}", engine::PROGRAM));
                engine.start_at = Some(Instant::now() + engine.restarts.after_exit());
            }
        }
    }

    /// Wakes the thread whenever a file in `folder` is written: the
    /// engine's frame is, once, after each start.
    fn watch_frames(self: &Arc<Self>, folder: &Path) -> std::io::Result<inotify::Stopper> {
        let watcher = inotify::Inotify::new(libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO)?;
        watcher.watch(folder)?;
        let stopper = watcher.stopper();
        let shared = Arc::clone(self);
        let frame = engine::frame_file(folder);
        std::thread::spawn(move || {
            while let Ok(Some(changes)) = watcher.read() {
                if changes.overflowed || changes.found.iter().any(|change| change.path == frame) {
                    lock(&shared.state).frame_written = true;
                    shared.wake.notify_all();
                }
            }
        });
        Ok(stopper)
    }

    /// Starts `command` as the program `running` picks out of the state,
    /// and notes when it exits: a crash unless it was asked to end.
    fn spawn(
        self: &Arc<Self>,
        state: &mut State,
        mut command: Command,
        running: fn(&mut State) -> &mut Running,
        name: &'static str,
        logger: &Arc<Logger>,
    ) -> std::io::Result<()> {
        let shared = Arc::clone(self);
        let exit_logger = Arc::clone(logger);
        // This thread, the spawning one, runs until `stop`.
        let pid = child::watch(&mut command, move |gone| {
            let mut state = lock(&shared.state);
            let program = running(&mut state);
            program.pid = None;
            let asked = std::mem::take(&mut program.ending);
            program.crashed = !asked;
            state.changed |= asked;
            exit_logger.info(LOG_SOURCE, &format!("{name} exited ({})", gone.reap()));
            shared.wake.notify_all();
        })?;
        running(state).pid = Some(pid);
        logger.info(LOG_SOURCE, &format!("started {name} (pid {pid})"));
        Ok(())
    }

    /// Waits until the daemon answers, or exits; each output's size in
    /// pixels. Waits on (as long as it would for an answer) while it hasn't
    /// sized each of `outputs` yet, so their first images can be stills
    /// that fit.
    fn wait_ready(&self, path_var: &str, outputs: &[&String]) -> Result<BTreeMap<String, (u32, u32)>, String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut answered = None;
        loop {
            if lock(&self.state).stopped {
                return Err("novad is stopping".into());
            }
            let answer = query(path_var);
            if let Some(output) = answer.as_ref().ok().filter(|output| output.status.success()) {
                let found = sizes(&String::from_utf8_lossy(&output.stdout));
                if outputs.iter().all(|name| found.contains_key(*name)) || Instant::now() >= deadline {
                    return Ok(found);
                }
                answered = Some(found);
                std::thread::sleep(READY_POLL);
                continue;
            }
            if lock(&self.state).daemon.pid.is_none() {
                return Err("it exited".into());
            }
            if Instant::now() >= deadline {
                // It answered before: it's up, just not sized yet.
                if let Some(found) = answered {
                    return Ok(found);
                }
                return Err(match answer {
                    Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                    Err(e) => e.to_string(),
                });
            }
            std::thread::sleep(READY_POLL);
        }
    }
}

/// Sends `wanted`'s images that awww doesn't show yet.
/// Whether every image it sent was shown.
/// Checks `stopped` before each `awww img`.
fn show(
    wanted: &Choice<PathBuf>,
    outputs: &Outputs,
    shown: &mut Shown,
    path_var: &str,
    logger: &Logger,
    stopped: &dyn Fn() -> bool,
) -> bool {
    match outputs {
        Outputs::Unknown => true,
        Outputs::Known(names) => {
            // One fade for every output that changes to the same image.
            let mut changes: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
            for (name, path) in wanted.per_output(names) {
                if shown.outputs.get(&name) != Some(&path) {
                    changes.entry(path).or_default().push(name);
                }
            }
            let mut all_shown = true;
            for (path, names) in changes {
                if stopped() {
                    return true;
                }
                match send_image(&path, Some(&names), path_var) {
                    Ok(()) => shown.outputs.extend(names.into_iter().map(|name| (name, path.clone()))),
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("can't show {}: {e}", path.display()));
                        all_shown = false;
                    }
                }
            }
            all_shown
        }
        Outputs::Unfollowed => {
            if shown.unfollowed.as_ref() == Some(wanted) {
                return true;
            }
            let every = wanted.path.iter().map(|path| (path, None));
            let own = wanted.outputs.iter().map(|(name, path)| (path, Some(vec![name.clone()])));
            let mut all_shown = true;
            for (path, names) in every.chain(own) {
                if stopped() {
                    return true;
                }
                if let Err(e) = send_image(path, names.as_deref(), path_var) {
                    logger.warn(LOG_SOURCE, &format!("can't show {}: {e}", path.display()));
                    all_shown = false;
                }
            }
            shown.unfollowed = all_shown.then(|| wanted.clone());
            all_shown
        }
    }
}

/// `awww query`: what the daemon shows on each output, once it answers.
fn query(path_var: &str) -> std::io::Result<std::process::Output> {
    let mut query = awww(path_var);
    query.args(["query", "--namespace", NAMESPACE]);
    child::output_within(query, QUERY_TIMEOUT)
}

/// Each output's size in pixels, as `awww query` says now.
fn query_sizes(path_var: &str) -> Option<BTreeMap<String, (u32, u32)>> {
    let output = query(path_var).ok().filter(|output| output.status.success())?;
    Some(sizes(&String::from_utf8_lossy(&output.stdout)))
}

/// Each output's size in pixels from `awww query`'s answer: a line per
/// output, `[<namespace>: ]<name>: <width>x<height>, scale: <scale>, …`,
/// with the size of its surface before scaling. One not sized yet (0x0) is
/// left out.
fn sizes(answer: &str) -> BTreeMap<String, (u32, u32)> {
    let size = |line: &str| {
        let (front, back) = line.split_once(", scale: ")?;
        let scale: f64 = back.split(',').next()?.trim().parse().ok()?;
        let (front, dim) = front.rsplit_once(": ")?;
        let name = front.rsplit_once(": ").map_or(front, |(_, name)| name);
        let (width, height) = dim.split_once('x')?;
        // Rounded as awww rounds it.
        let pixels = |length: &str| Some((f64::from(length.parse::<u32>().ok()?) * scale).round() as u32);
        Some((name.to_owned(), (pixels(width)?, pixels(height)?))).filter(|(_, (width, height))| width * height > 0)
    };
    answer.lines().filter_map(size).collect()
}

/// Cuts the engine's frame into stills for next time, once it's been
/// `written`. Given up on once it's waited long enough, or when it can't be
/// cut up. Checks `stopped` before each change to the state folder.
fn keep_stills(
    engine: &mut EngineRun,
    written: bool,
    sizes: &BTreeMap<String, (u32, u32)>,
    places: &Places,
    logger: &Logger,
    stopped: &dyn Fn() -> bool,
) {
    let Some(frame) = engine.frame.as_ref() else {
        return;
    };
    let file = engine::frame_file(&places.engine);
    let saved = match written {
        false => Err(engine::Unsaved::NotYet),
        true => match std::fs::read(&file) {
            Ok(bytes) => engine::save_stills(&bytes, &frame.screens, sizes, &places.engine, stopped),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(engine::Unsaved::NotYet),
            Err(e) => Err(engine::Unsaved::Never(format!("can't read {}: {e}", file.display()))),
        },
    };
    let why = match saved {
        Err(engine::Unsaved::NotYet) if Instant::now() < frame.until => return,
        Ok(()) => None,
        Err(engine::Unsaved::NotYet) => Some("it didn't write one".to_owned()),
        Err(engine::Unsaved::Never(why)) => Some(why),
    };
    match why {
        None => logger.log(LogLevel::Debug, LOG_SOURCE, "kept stills of the engine's frame"),
        Some(why) => logger.info(LOG_SOURCE, &format!("no stills from the engine's frame: {why}")),
    }
    engine.frame = None;
    if !stopped() {
        let _ = std::fs::remove_file(&file);
    }
}

/// `awww img` for `path` on `outputs`, or every output.
fn send_image(path: &Path, outputs: Option<&[String]>, path_var: &str) -> Result<(), String> {
    let mut command = awww(path_var);
    command.args([
        "img",
        "--namespace",
        NAMESPACE,
        "--transition-type",
        TRANSITION,
        "--transition-duration",
        TRANSITION_SECONDS,
        "--transition-fps",
        TRANSITION_FPS,
        "--resize",
        RESIZE,
    ]);
    if let Some(outputs) = outputs {
        command.arg("--outputs").arg(outputs.join(","));
    }
    command.arg(path);
    let output = child::output_within(command, IMAGE_TIMEOUT).map_err(|e| e.to_string())?;
    match output.status.success() {
        true => Ok(()),
        false => Err(format!("{CLIENT} {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
    }
}

fn awww(path_var: &str) -> Command {
    let mut command = Command::new(CLIENT);
    command.env("PATH", path_var);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_hub::Hub;
    use nova_test_support::{TempDir, eventually, wait_until};

    const WAIT: Duration = Duration::from_secs(10);
    const WORKSHOP_ID: &str = "2861349227";

    struct Fixture {
        dir: TempDir,
        logger: Arc<Logger>,
        /// The fakes and the `sleep` they run: a build sandbox has no
        /// /usr/bin, and a fake daemon that can't sleep exits at once.
        path: String,
    }

    impl Fixture {
        /// Stand-ins for awww in `<dir>/bin`: the daemon writes its pid to
        /// `<dir>/runs` each time it starts, then sleeps; the client writes
        /// its arguments to `<dir>/awww` and succeeds, answering `query`
        /// with `<dir>/query` (DP-1 and DP-2, 1x1 pixels).
        /// linux-wallpaperengine's writes its pid and arguments to
        /// `<dir>/engine`, copies `<dir>/frame.png` to its `--screenshot`
        /// file, then sleeps. Two
        /// images and a video in `<dir>`, and a downloaded Workshop item with
        /// a preview in `<dir>/home`'s Steam library.
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("wallpaper-{name}"));
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            let runs = dir.join("runs");
            nova_test_support::write_program(
                &dir.join("bin"),
                DAEMON,
                &format!("echo $$ >> '{}'\nexec sleep 30", runs.display()),
            );
            nova_test_support::write_program(
                &dir.join("bin"),
                CLIENT,
                // Fails while there's a file `fail`, as for an output awww
                // hasn't bound yet.
                &format!(
                    "[ \"$1\" = img ] && [ -e '{}' ] && exit 1\necho \"$*\" >> '{}'\n\
                     [ \"$1\" = query ] && [ -e '{query}' ] && cat '{query}'\nexit 0",
                    dir.join("fail").display(),
                    dir.join("awww").display(),
                    query = dir.join("query").display(),
                ),
            );
            nova_test_support::write_program(
                &dir.join("bin"),
                engine::PROGRAM,
                &format!(
                    "echo $$ \"$*\" >> '{}'\nwhile [ $# -gt 0 ]; do\n\
                     [ \"$1\" = --screenshot ] && [ -e '{frame}' ] && cp '{frame}' \"$2\"\nshift\ndone\n\
                     exec sleep 30",
                    dir.join("engine").display(),
                    frame = dir.join("frame.png").display(),
                ),
            );
            let item = dir.join(format!("home/.local/share/Steam/steamapps/workshop/content/431960/{WORKSHOP_ID}"));
            std::fs::create_dir_all(&item).unwrap();
            std::fs::write(item.join(source::PROJECT_FILE), r#"{"type": "scene", "preview": "preview.png"}"#).unwrap();
            for image in [dir.join("dusk.png"), dir.join("tide.png"), item.join("preview.png")] {
                let file = std::fs::File::create(image).unwrap();
                let mut encoder = png::Encoder::new(file, 1, 1);
                encoder.set_color(png::ColorType::Rgb);
                encoder.write_header().unwrap().write_image_data(&[0, 0, 0]).unwrap();
            }
            std::fs::write(dir.join("rain.webm"), "frames").unwrap();
            let sizes =
                ["DP-1", "DP-2"].map(|name| format!("nova: {name}: 1x1, scale: 1, currently displaying: color: 0\n"));
            std::fs::write(dir.join("query"), sizes.concat()).unwrap();
            let logger =
                Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap());
            let path = nova_test_support::fake_path(&dir.join("bin"), &["sleep", "cp", "cat"]);
            Self { dir, logger, path }
        }

        fn start(&self) -> Arc<Wallpaper> {
            Wallpaper::start(
                &self.dir.join("state"),
                Some(self.dir.join("home")),
                self.path.clone(),
                true,
                Arc::clone(&self.logger),
            )
        }

        fn image(&self, name: &str) -> String {
            self.dir.join(name).display().to_string()
        }

        /// `[wallpaper] path` set to the image `name`.
        fn config(&self, name: &str) -> WallpaperConfig {
            WallpaperConfig { path: Some(self.image(name)), ..WallpaperConfig::default() }
        }

        /// The daemon's pids, once it started `count` times.
        fn runs(&self, count: usize) -> Vec<i32> {
            eventually(WAIT, &format!("{DAEMON} to start {count} times"), || {
                let runs: Vec<i32> = std::fs::read_to_string(self.dir.join("runs"))
                    .unwrap_or_default()
                    .lines()
                    .map(|line| line.parse().unwrap())
                    .collect();
                (runs.len() >= count).then_some(runs)
            })
        }

        /// The outputs and image of each `awww img`, once there are `count`.
        fn images(&self, count: usize) -> Vec<(String, String)> {
            eventually(WAIT, &format!("{count} images sent"), || {
                let sent = self.sent();
                (sent.len() >= count).then_some(sent)
            })
        }

        /// Waits until the last image sent to each output is `expected`'s.
        /// An `awww img` that novad saw fail under load is sent again, so
        /// the order of the images may vary.
        fn shows(&self, expected: &[(&str, &str)]) {
            wait_until(WAIT, &format!("outputs to show {expected:?}"), || {
                let sent = self.sent();
                // Later images win.
                let last: BTreeMap<&str, &str> = sent
                    .iter()
                    .flat_map(|(outputs, image)| outputs.split(',').map(move |output| (output, image.as_str())))
                    .collect();
                expected.iter().all(|(output, image)| last.get(*output) == Some(image))
            });
        }

        fn item(&self) -> String {
            let workshop = "home/.local/share/Steam/steamapps/workshop/content/431960";
            self.dir.join(workshop).join(WORKSHOP_ID).display().to_string()
        }

        /// Each linux-wallpaperengine started: its pid, and the project it
        /// showed on each output.
        fn engine_runs(&self) -> Vec<(i32, Vec<(String, String)>)> {
            std::fs::read_to_string(self.dir.join("engine"))
                .unwrap_or_default()
                .lines()
                .map(|line| {
                    let words: Vec<&str> = line.split(' ').collect();
                    let screens = words
                        .windows(4)
                        .filter(|four| four[0] == "--screen-root" && four[2] == "--bg")
                        .map(|four| (four[1].to_owned(), four[3].to_owned()))
                        .collect();
                    (words[0].parse().unwrap(), screens)
                })
                .collect()
        }

        /// Waits until linux-wallpaperengine started `count` times, the last
        /// one runs and shows `expected`, and no other runs; its pid.
        fn engine_shows(&self, count: usize, expected: &[(&str, &str)]) -> i32 {
            let expected: Vec<(String, String)> =
                expected.iter().map(|(output, project)| ((*output).to_owned(), (*project).to_owned())).collect();
            eventually(WAIT, &format!("{} to show {expected:?}", engine::PROGRAM), || {
                let runs = self.engine_runs();
                let (last, screens) = runs.last().filter(|_| runs.len() >= count)?;
                let others_gone = runs[..runs.len() - 1].iter().all(|(pid, _)| !alive(*pid));
                (*screens == expected && alive(*last) && others_gone).then_some(*last)
            })
        }

        fn sent(&self) -> Vec<(String, String)> {
            std::fs::read_to_string(self.dir.join("awww"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.starts_with("img "))
                .map(|line| {
                    let words: Vec<&str> = line.split(' ').collect();
                    let outputs = words.iter().position(|word| *word == "--outputs").map(|at| words[at + 1]);
                    (outputs.unwrap_or("every").to_owned(), words.last().unwrap().to_string())
                })
                .collect()
        }
    }

    fn alive(pid: i32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
            && !std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default().contains(") Z ")
    }

    fn outputs(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn nothing_runs_until_an_image_is_set() {
        let fixture = Fixture::new("unset");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.configure(true, false, &WallpaperConfig::default());
        std::thread::sleep(Duration::from_millis(300));
        assert!(!fixture.dir.join("runs").exists(), "a wallpaper tool of the user's keeps the screen");
        wallpaper.set(&fixture.image("dusk.png"), None).unwrap();
        fixture.runs(1);
        assert_eq!(fixture.images(1), [("DP-1,DP-2".into(), fixture.image("dusk.png"))]);
        let line = std::fs::read_to_string(fixture.dir.join("awww")).unwrap();
        assert!(line.contains("--namespace nova") && line.contains("--transition-type fade"), "{line}");
        wallpaper.stop();
    }

    #[test]
    fn only_outputs_that_change_get_an_image() {
        let fixture = Fixture::new("changes");
        let wallpaper = fixture.start();
        wallpaper.configure(true, false, &fixture.config("dusk.png"));
        std::thread::sleep(Duration::from_millis(300));
        assert!(fixture.sent().is_empty(), "nothing before the outputs are known");
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        let (dusk, tide) = (fixture.image("dusk.png"), fixture.image("tide.png"));
        assert_eq!(fixture.images(1), [("DP-1,DP-2".into(), dusk.clone())], "one fade for both");
        wallpaper.set(&tide, Some("DP-2")).unwrap();
        fixture.shows(&[("DP-1", &dusk), ("DP-2", &tide)]);
        // Plugged in: the new output gets its image.
        wallpaper.outputs(outputs(&["DP-1", "DP-2", "HDMI-A-1"]));
        fixture.shows(&[("DP-1", &dusk), ("DP-2", &tide), ("HDMI-A-1", &dusk)]);
        // Unplugged and back: it gets it again.
        let sent = fixture.sent().len();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.outputs(outputs(&["DP-1", "DP-2", "HDMI-A-1"]));
        wait_until(WAIT, "HDMI-A-1 to get its image again", || {
            fixture.sent()[sent..]
                .iter()
                .any(|(again, image)| again.split(',').any(|output| output == "HDMI-A-1") && *image == dusk)
        });
        assert_eq!(wallpaper.current().unwrap().outputs["DP-2"], tide);
        assert_eq!(fixture.runs(1).len(), 1);
        wallpaper.stop();
    }

    #[test]
    fn the_choice_lasts_until_the_config_changes() {
        let fixture = Fixture::new("saved");
        let config = fixture.config("dusk.png");
        let first = fixture.start();
        first.configure(true, false, &config);
        first.set(&fixture.image("tide.png"), None).unwrap();
        first.stop();
        // Started again with the same config: the choice stays.
        let again = fixture.start();
        again.configure(true, false, &config);
        assert_eq!(again.current().unwrap().path, Some(fixture.image("tide.png")));
        // The config changes: it wins, and the choice is gone.
        let edited = WallpaperConfig { outputs: [("DP-2".into(), fixture.image("tide.png"))].into(), ..config };
        again.configure(true, false, &edited);
        assert_eq!(again.current().unwrap().path, Some(fixture.image("dusk.png")));
        assert!(!fixture.dir.join("state").join(STATE_FILE).exists());
        again.stop();
        // So it's gone after a restart too.
        let last = fixture.start();
        last.configure(true, false, &edited);
        assert_eq!(last.current().unwrap().path, Some(fixture.image("dusk.png")));
        last.stop();
    }

    #[test]
    fn a_new_folder_keeps_the_choice() {
        let fixture = Fixture::new("folder");
        let config = fixture.config("dusk.png");
        let first = fixture.start();
        first.configure(true, false, &config);
        first.set(&fixture.image("tide.png"), None).unwrap();
        let moved = WallpaperConfig { folder: Some("/art".into()), ..config };
        first.configure(true, false, &moved);
        assert_eq!(first.current().unwrap().path, Some(fixture.image("tide.png")));
        first.stop();
        let again = fixture.start();
        again.configure(true, false, &moved);
        assert_eq!(again.current().unwrap().path, Some(fixture.image("tide.png")));
        again.stop();
    }

    #[test]
    fn the_config_changing_while_novad_is_off_wins() {
        let fixture = Fixture::new("offline-edit");
        let first = fixture.start();
        first.configure(true, false, &fixture.config("dusk.png"));
        first.set(&fixture.image("tide.png"), None).unwrap();
        first.stop();
        let again = fixture.start();
        again.configure(true, false, &WallpaperConfig::default());
        assert_eq!(again.current(), Ok(Wallpapers::default()));
        again.stop();
    }

    #[test]
    fn choosing_the_same_file_again_sends_it_again() {
        let fixture = Fixture::new("again");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.configure(true, false, &WallpaperConfig::default());
        let tide = fixture.image("tide.png");
        wallpaper.set(&tide, Some("DP-2")).unwrap();
        fixture.images(1);
        // A script rewrote the file and chose it again.
        wallpaper.set(&tide, Some("DP-2")).unwrap();
        assert_eq!(fixture.images(2)[1], ("DP-2".into(), tide.clone()));
        wallpaper.set(&tide, None).unwrap();
        assert_eq!(fixture.images(3)[2], ("DP-1,DP-2".into(), tide));
        wallpaper.stop();
    }

    #[test]
    fn a_failed_image_is_sent_again() {
        let fixture = Fixture::new("resend");
        std::fs::write(fixture.dir.join("fail"), "").unwrap();
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, false, &fixture.config("dusk.png"));
        fixture.runs(1);
        std::thread::sleep(Duration::from_millis(300));
        std::fs::remove_file(fixture.dir.join("fail")).unwrap();
        // Nothing else changes: it's sent again on its own.
        fixture.shows(&[("DP-1", &fixture.image("dusk.png"))]);
        assert_eq!(fixture.runs(1).len(), 1, "the daemon is fine");
        wallpaper.stop();
    }

    #[test]
    fn a_crashed_daemon_starts_again_and_gets_its_images_back() {
        let fixture = Fixture::new("crash");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, false, &fixture.config("dusk.png"));
        let first = fixture.runs(1)[0];
        fixture.images(1);
        // SAFETY: kill only sends a signal.
        unsafe { libc::kill(first, libc::SIGKILL) };
        let second = fixture.runs(2)[1];
        assert_ne!(first, second);
        assert_eq!(fixture.images(2)[1], ("DP-1".into(), fixture.image("dusk.png")));
        wallpaper.stop();
        wait_until(WAIT, "stopping to end it", || !alive(second));
    }

    #[test]
    fn the_daemon_stops_with_the_feature() {
        let fixture = Fixture::new("off");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, false, &fixture.config("dusk.png"));
        let pid = fixture.runs(1)[0];
        wallpaper.configure(false, false, &fixture.config("dusk.png"));
        wait_until(WAIT, "the daemon to stop", || !alive(pid));
        assert!(wallpaper.set(&fixture.image("tide.png"), None).unwrap_err().contains("isn't ready"));
        std::thread::sleep(Restarts::FIRST + Duration::from_millis(200));
        assert_eq!(fixture.runs(1).len(), 1, "not started again while off");
        wallpaper.stop();
    }

    #[test]
    fn a_bad_choice_is_refused_and_changes_nothing() {
        let fixture = Fixture::new("refused");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.configure(true, false, &fixture.config("dusk.png"));
        assert!(wallpaper.set("dusk.png", None).unwrap_err().contains("absolute"));
        assert!(wallpaper.set(&fixture.image("missing.jpg"), None).unwrap_err().contains("can't read"));
        let folder = wallpaper.set(&fixture.dir.display().to_string(), None).unwrap_err();
        assert!(folder.contains("isn't a Wallpaper Engine project"), "{folder}");
        std::fs::write(fixture.dir.join("notes.toml"), "path = \"dusk.png\"").unwrap();
        assert!(wallpaper.set(&fixture.image("notes.toml"), None).unwrap_err().contains("isn't an image, a video"));
        let unknown = wallpaper.set(&fixture.image("tide.png"), Some("HDMI-A-1")).unwrap_err();
        assert!(unknown.contains("DP-1, DP-2"), "{unknown}");
        assert!(wallpaper.set(&fixture.image("tide.png"), Some("DP-1,DP-2")).is_err());
        assert_eq!(wallpaper.current().unwrap().path, Some(fixture.image("dusk.png")));
        assert!(!fixture.dir.join("state").join(STATE_FILE).exists());
        wallpaper.stop();
    }

    #[test]
    fn a_dynamic_wallpaper_runs_over_its_preview() {
        let fixture = Fixture::new("dynamic");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.configure(true, true, &WallpaperConfig { path: Some(WORKSHOP_ID.into()), ..Default::default() });
        let (item, preview) = (fixture.item(), format!("{}/preview.png", fixture.item()));
        fixture.shows(&[("DP-1", &preview), ("DP-2", &preview)]);
        let first = fixture.engine_shows(1, &[("DP-1", &item), ("DP-2", &item)]);
        assert_eq!(wallpaper.current().unwrap().path.as_deref(), Some(WORKSHOP_ID));
        // One output gets a still image: the engine keeps only the other.
        let tide = fixture.image("tide.png");
        wallpaper.set(&tide, Some("DP-2")).unwrap();
        fixture.shows(&[("DP-1", &preview), ("DP-2", &tide)]);
        let second = fixture.engine_shows(2, &[("DP-1", &item)]);
        assert_ne!(first, second);
        // No dynamic wallpaper left: it ends, and isn't started again.
        wallpaper.set(&fixture.image("dusk.png"), None).unwrap();
        wait_until(WAIT, "the engine to end", || !alive(second));
        std::thread::sleep(Restarts::FIRST + Duration::from_millis(200));
        assert_eq!(fixture.engine_runs().len(), 2, "asked to end isn't a crash");
        assert_eq!(fixture.runs(1).len(), 1, "awww ran all along");
        wallpaper.stop();
    }

    #[test]
    fn a_dynamic_wallpaper_starts_from_its_last_frame_next_time() {
        let fixture = Fixture::new("stills");
        // With Steam's assets, nothing else makes the folder the frame goes to.
        std::fs::create_dir_all(fixture.dir.join("home/.local/share/Steam/steamapps/common/wallpaper_engine/assets"))
            .unwrap();
        // DP-2 is scaled: 1x1 before, 2x2 in pixels. The engine draws both
        // side by side.
        let query = |dp1: &str| {
            format!(
                "nova: DP-1: {dp1}, scale: 1, currently displaying: image: x\nnova: DP-2: 1x1, scale: 2, currently displaying: image: x\n"
            )
        };
        std::fs::write(fixture.dir.join("query"), query("3x2")).unwrap();
        let file = std::fs::File::create(fixture.dir.join("frame.png")).unwrap();
        let mut encoder = png::Encoder::new(file, 5, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.write_header().unwrap().write_image_data(&[9; 5 * 2 * 3]).unwrap();
        let config = WallpaperConfig { path: Some(WORKSHOP_ID.into()), ..Default::default() };
        let start = || {
            let wallpaper = fixture.start();
            wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
            wallpaper.configure(true, true, &config);
            wallpaper
        };
        let (item, preview) = (fixture.item(), format!("{}/preview.png", fixture.item()));
        let engine_folder = fixture.dir.join("state").join("wallpaper-engine");
        let still = |size| engine::still(Path::new(&item), size, &engine_folder);
        // The first time, only the preview.
        let first = start();
        fixture.shows(&[("DP-1", &preview), ("DP-2", &preview)]);
        fixture.engine_shows(1, &[("DP-1", &item), ("DP-2", &item)]);
        wait_until(WAIT, "the frame to be kept", || still((3, 2)).is_some() && still((2, 2)).is_some());
        first.stop();
        // Next time, each output starts from the frame, as big as it is.
        let sent = fixture.sent().len();
        let again = start();
        let (big, small) = (still((3, 2)).unwrap().display().to_string(), still((2, 2)).unwrap().display().to_string());
        fixture.shows(&[("DP-1", &big), ("DP-2", &small)]);
        assert!(fixture.sent()[sent..].iter().all(|(_, image)| *image != preview), "no preview first");
        again.stop();
        // DP-1 got bigger: its still doesn't fit, so the preview it is.
        std::fs::write(fixture.dir.join("query"), query("6x4")).unwrap();
        let bigger = start();
        fixture.shows(&[("DP-1", &preview), ("DP-2", &small)]);
        // A still image everywhere: the frames go.
        bigger.set(&fixture.image("dusk.png"), None).unwrap();
        wait_until(WAIT, "the unused stills to go", || !engine::has_stills(Path::new(&item), &engine_folder));
        bigger.stop();
    }

    #[test]
    fn a_video_on_two_outputs_gets_stills_too() {
        let fixture = Fixture::new("video-stills");
        // DP-1 and DP-2 are 1x1 pixels: side by side, 2x1.
        let file = std::fs::File::create(fixture.dir.join("frame.png")).unwrap();
        let mut encoder = png::Encoder::new(file, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.write_header().unwrap().write_image_data(&[9; 2 * 3]).unwrap();
        let config = WallpaperConfig { path: Some(fixture.image("rain.webm")), ..Default::default() };
        let first = fixture.start();
        first.outputs(outputs(&["DP-1", "DP-2"]));
        first.configure(true, true, &config);
        let project = eventually(WAIT, "the engine to start", || fixture.engine_runs().first()?.1.first().cloned()).1;
        let engine_folder = fixture.dir.join("state").join("wallpaper-engine");
        // No preview to show, but awww runs to say the outputs' sizes.
        wait_until(WAIT, "the frame to be kept", || {
            engine::still(Path::new(&project), (1, 1), &engine_folder).is_some()
        });
        assert!(fixture.sent().is_empty(), "nothing to show the first time");
        first.stop();
        let again = fixture.start();
        again.outputs(outputs(&["DP-1", "DP-2"]));
        again.configure(true, true, &config);
        let still = engine::still(Path::new(&project), (1, 1), &engine_folder).unwrap().display().to_string();
        fixture.shows(&[("DP-1", &still), ("DP-2", &still)]);
        again.stop();
    }

    #[test]
    fn awww_says_each_outputs_size_in_pixels() {
        let answer = "nova: DP-1: 2560x1440, scale: 1, currently displaying: image: /a: b.png\n\
                      eDP-1: 1280x800, scale: 1.25, currently displaying: color: 000000\n\
                      nova: HDMI-A-1: 0x0, scale: 1, currently displaying: color: 000000\nodd line";
        let sizes = sizes(answer);
        assert_eq!(sizes, [("DP-1".to_owned(), (2560, 1440)), ("eDP-1".to_owned(), (1600, 1000))].into());
    }

    #[test]
    fn the_engine_starts_again_after_a_crash_and_after_awww_does() {
        let fixture = Fixture::new("engine-crash");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, true, &WallpaperConfig::default());
        wallpaper.set(WORKSHOP_ID, None).unwrap();
        let item = fixture.item();
        let first = fixture.engine_shows(1, &[("DP-1", &item)]);
        // SAFETY: kill only sends a signal.
        unsafe { libc::kill(first, libc::SIGKILL) };
        let second = fixture.engine_shows(2, &[("DP-1", &item)]);
        assert_ne!(first, second);
        // awww started again draws over it, so it starts again too.
        let daemon = fixture.runs(1)[0];
        // SAFETY: kill only sends a signal.
        unsafe { libc::kill(daemon, libc::SIGKILL) };
        fixture.runs(2);
        let third = fixture.engine_shows(3, &[("DP-1", &item)]);
        assert_ne!(second, third);
        wallpaper.stop();
        wait_until(WAIT, "stopping to end it", || !alive(third));
    }

    #[test]
    fn a_video_plays_through_a_project_of_its_own() {
        let fixture = Fixture::new("video");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, true, &WallpaperConfig::default());
        wallpaper.set(&fixture.image("rain.webm"), None).unwrap();
        let project = eventually(WAIT, "the engine to start", || fixture.engine_runs().first()?.1.first().cloned()).1;
        let videos = fixture.dir.join("state").join("wallpaper-engine").join("videos");
        assert!(Path::new(&project).starts_with(&videos), "{project}");
        assert!(Path::new(&project).join(source::PROJECT_FILE).is_file());
        assert!(fixture.sent().is_empty(), "a video has no preview for awww");
        wallpaper.set(&fixture.image("dusk.png"), None).unwrap();
        fixture.shows(&[("DP-1", &fixture.image("dusk.png"))]);
        wait_until(WAIT, "the unused project to go", || !Path::new(&project).exists());
        wallpaper.stop();
    }

    #[test]
    fn dynamic_wallpapers_need_their_feature() {
        let fixture = Fixture::new("engine-off");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, false, &WallpaperConfig { path: Some(WORKSHOP_ID.into()), ..Default::default() });
        // The config's stays still: only its preview shows.
        fixture.shows(&[("DP-1", &format!("{}/preview.png", fixture.item()))]);
        let refused = wallpaper.set(&fixture.image("rain.webm"), None).unwrap_err();
        assert!(refused.contains(ENGINE_FEATURE_ID), "{refused}");
        let missing = wallpaper.set("1", None).unwrap_err();
        assert!(missing.contains("isn't downloaded"), "{missing}");
        std::thread::sleep(Duration::from_millis(300));
        assert!(fixture.engine_runs().is_empty());
        // Only dynamic ones, with the still images off.
        wallpaper.configure(false, true, &WallpaperConfig::default());
        assert!(wallpaper.set(&fixture.image("dusk.png"), None).unwrap_err().contains("isn't ready"));
        wallpaper.set(WORKSHOP_ID, None).unwrap();
        fixture.engine_shows(1, &[("DP-1", &fixture.item())]);
        wallpaper.stop();
    }

    #[test]
    fn images_come_back_after_awww_was_ended() {
        let fixture = Fixture::new("ended");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        let dusk = fixture.config("dusk.png");
        wallpaper.configure(true, false, &dusk);
        let first = fixture.runs(1)[0];
        fixture.images(1);
        // Turned off and on: the new daemon has no image until it's sent.
        wallpaper.configure(false, false, &dusk);
        wait_until(WAIT, "the daemon to end", || !alive(first));
        wallpaper.configure(true, false, &dusk);
        fixture.runs(2);
        assert_eq!(fixture.images(2)[1], ("DP-1".into(), fixture.image("dusk.png")));
        wallpaper.stop();
    }
}
