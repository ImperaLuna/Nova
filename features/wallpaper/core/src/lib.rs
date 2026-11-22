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
mod drawing;
mod engine;
pub mod folder;
mod plan;
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
use drawing::*;
use plan::*;

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

/// What each step of the drive loop works with: where things are, the
/// log, and whether novad is stopping (checked before each change).
#[derive(Clone, Copy)]
struct Drive<'a> {
    places: &'a Places,
    logger: &'a Arc<Logger>,
    stopped: &'a dyn Fn() -> bool,
}

/// A program the drive loop runs: its name, and where the state keeps it.
#[derive(Clone, Copy)]
struct Program {
    name: &'static str,
    running: fn(&mut State) -> &mut Running,
}

const AWWW_DAEMON: Program = Program { name: DAEMON, running: |state| &mut state.daemon };
const ENGINE: Program = Program { name: engine::PROGRAM, running: |state| &mut state.engine };

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
        let stopped = || lock(&self.state).stopped;
        let drive = Drive { places, logger, stopped: &stopped };
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
                keep_stills(&mut engine, written, &awww.sizes, drive);
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
            let plan = Plan::new(&choice, &outputs, (images_ready, engine_ready), drive);
            self.drive_awww(&plan.images, &outputs, &mut awww, drive);
            self.drive_engine(&plan.screens, &mut engine, awww.starts, drive);
        }
    }

    /// Runs awww's daemon while it has images to show, and sends the ones
    /// it doesn't show yet.
    fn drive_awww(self: &Arc<Self>, images: &Choice<Picture>, outputs: &Outputs, awww: &mut Awww, drive: Drive) {
        let Drive { places, logger, .. } = drive;
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
            if let Err(e) = self.spawn(&mut state, command, AWWW_DAEMON, logger) {
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
        if show(&images, outputs, &mut awww.shown, drive) {
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
        drive: Drive,
    ) {
        let Drive { places, logger, .. } = drive;
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
            self.spawn(&mut state, command, ENGINE, logger).map_err(|e| e.to_string())
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

    /// Starts `command` as `program`, and notes in the state when it exits:
    /// a crash unless it was asked to end.
    fn spawn(
        self: &Arc<Self>,
        state: &mut State,
        mut command: Command,
        program: Program,
        logger: &Arc<Logger>,
    ) -> std::io::Result<()> {
        let Program { name, running } = program;
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
                nova_base::backoff::wait_out(READY_POLL);
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
            nova_base::backoff::wait_out(READY_POLL);
        }
    }
}

#[cfg(test)]
mod tests;
