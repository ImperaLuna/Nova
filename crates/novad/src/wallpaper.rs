//! Wallpapers (feature `wallpaper`): an image on every output, or one of its
//! own on an output, set in config.toml's `[wallpaper]` or with
//! `nova wallpaper`, which wins until that section changes.
//!
//! awww draws them: novad runs its daemon in a namespace of Nova's own (so a
//! copy the user runs never clashes with it), without its cache, so what it
//! shows only ever comes from novad, and starts it again when it exits,
//! waiting longer each time it keeps failing. Nothing runs while no image is
//! set, so a wallpaper tool of the user's own keeps working. A new image
//! fades in on the outputs that change; one fade covers every output that
//! gets the same image. An output that's plugged in (or comes back) gets its
//! image as soon as the compositor reports it.
//!
//! One thread owns the daemon and sends the images; everything else changes
//! what it wants under the lock and wakes it.

use crate::backoff::Backoff;
use crate::config::{self, WallpaperConfig};
use crate::logger::Logger;
use crate::{child, lock, state_file};
use nova_protocol::Wallpapers;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "wallpaper";
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
/// Waits before starting the daemon again: doubling from the first while it
/// keeps failing, back to the first once it ran this long.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);
const RAN_FINE: Duration = Duration::from_secs(30);

/// The image for every output and the outputs with one of their own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    outputs: BTreeMap<String, PathBuf>,
}

impl Choice {
    fn is_empty(&self) -> bool {
        self.path.is_none() && self.outputs.is_empty()
    }

    /// The image each of `names` shows; one with none is left out.
    fn per_output(&self, names: &[String]) -> BTreeMap<String, PathBuf> {
        names
            .iter()
            .filter_map(|name| Some((name.clone(), self.outputs.get(name).or(self.path.as_ref())?.clone())))
            .collect()
    }

    /// The choice from config.toml's `[wallpaper]`, leaving out (and logging)
    /// paths that aren't absolute.
    fn from_config(config: &WallpaperConfig, logger: &Logger) -> Self {
        let resolve = |setting: String, path: &str| {
            config::user_path(&setting, path).map_err(|e| logger.warn(LOG_SOURCE, &format!("{e}, ignoring it"))).ok()
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
        let text = |path: &PathBuf| path.display().to_string();
        Wallpapers {
            path: self.path.as_ref().map(text),
            outputs: self.outputs.iter().map(|(name, path)| (name.clone(), text(path))).collect(),
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

/// The outputs to show images on.
#[derive(Debug, Clone, PartialEq)]
enum Outputs {
    /// As the compositor last reported them.
    Known(Vec<String>),
    /// The compositor hasn't reported them yet: nothing is sent until it
    /// does, so every output gets its own image in one go.
    Unknown,
    /// novad doesn't follow this compositor: images go to whatever outputs
    /// awww sees.
    Unfollowed,
}

pub struct Wallpaper {
    shared: Arc<Shared>,
    state_file: PathBuf,
    logger: Arc<Logger>,
}

struct Shared {
    state: Mutex<State>,
    /// Wakes the thread on any change to `state`.
    wake: Condvar,
}

struct State {
    /// The feature is ready.
    ready: bool,
    /// The last `[wallpaper]` seen, `None` before the first.
    config: Option<WallpaperConfig>,
    from_config: Choice,
    /// `nova wallpaper`'s choice, over the config's.
    chosen: Option<Choice>,
    outputs: Outputs,
    /// Outputs unplugged since the thread last looked: awww forgot their
    /// image, even when they're back by then.
    gone: BTreeSet<String>,
    /// The running daemon's pid, for `stop` and to end it when nothing is
    /// wanted. Forgotten before it's reaped.
    daemon: Option<u32>,
    /// Since the thread last looked: what's wanted changed, or the daemon
    /// exited.
    changed: bool,
    exited: bool,
    stopped: bool,
}

impl State {
    /// What should show, when anything should.
    fn wanted(&self) -> Option<Choice> {
        let choice = self.chosen.as_ref().unwrap_or(&self.from_config);
        (self.ready && !choice.is_empty()).then(|| choice.clone())
    }

    fn current(&self) -> &Choice {
        self.chosen.as_ref().unwrap_or(&self.from_config)
    }
}

impl Wallpaper {
    /// Starts its thread, which runs nothing until [`configure`](Self::configure)
    /// says the feature is ready and an image is set. `follows_compositor`:
    /// [`outputs`](Self::outputs) will report the outputs.
    pub fn start(state_dir: &Path, path_var: String, follows_compositor: bool, logger: Arc<Logger>) -> Arc<Self> {
        let state = State {
            ready: false,
            config: None,
            from_config: Choice::default(),
            chosen: None,
            outputs: if follows_compositor { Outputs::Unknown } else { Outputs::Unfollowed },
            gone: BTreeSet::new(),
            daemon: None,
            changed: false,
            exited: false,
            stopped: false,
        };
        let shared = Arc::new(Shared { state: Mutex::new(state), wake: Condvar::new() });
        let running = Arc::clone(&shared);
        let thread_logger = Arc::clone(&logger);
        std::thread::spawn(move || running.run(&path_var, &thread_logger));
        Arc::new(Self { shared, state_file: state_dir.join(STATE_FILE), logger })
    }

    /// Whether the feature is ready, and config.toml's `[wallpaper]`. Called
    /// at start and on every reload. A changed section drops what
    /// `nova wallpaper` chose; at start, so does one changed while novad
    /// wasn't running.
    pub fn configure(&self, ready: bool, config: &WallpaperConfig) {
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
        state.ready = ready;
        self.changed(state);
    }

    /// The compositor's outputs, whenever they change.
    pub fn outputs(&self, names: Vec<String>) {
        let mut state = lock(&self.shared.state);
        if let Outputs::Known(before) = &state.outputs {
            let gone: Vec<String> = before.iter().filter(|name| !names.contains(name)).cloned().collect();
            state.gone.extend(gone);
        }
        let outputs = Outputs::Known(names);
        if state.outputs != outputs {
            state.outputs = outputs;
            self.changed(state);
        }
    }

    /// Shows the image at `path` on every output, or only on `output`, and
    /// remembers it (`nova wallpaper`).
    pub fn set(&self, path: &str, output: Option<&str>) -> Result<(), String> {
        let path = readable_image(path)?;
        let mut state = lock(&self.shared.state);
        if !state.ready {
            return Err(crate::features::not_ready(FEATURE_ID));
        }
        let mut choice = state.current().clone();
        match output {
            Some(name) => {
                check_output(name, &state.outputs)?;
                choice.outputs.insert(name.to_owned(), path);
            }
            None => choice = Choice { path: Some(path), outputs: BTreeMap::new() },
        }
        let saved = Saved { config: state.config.clone().unwrap_or_default(), choice };
        state_file::save(&self.state_file, &saved)
            .map_err(|e| format!("can't save the choice to {}: {e}", self.state_file.display()))?;
        state.chosen = Some(saved.choice);
        self.changed(state);
        Ok(())
    }

    /// What shows now, or would once the feature is ready.
    pub fn current(&self) -> Wallpapers {
        lock(&self.shared.state).current().wire()
    }

    /// Ends the daemon and the thread, for a novad that ends without exiting
    /// the process ([`crate::InProcess`]).
    pub fn stop(&self) {
        let mut state = lock(&self.shared.state);
        state.stopped = true;
        // Under the lock: the thread forgets the pid under it before
        // reaping, so it's still the daemon's (at worst a zombie).
        if let Some(pid) = state.daemon {
            terminate(pid);
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

/// `path` if it's an absolute path to a file novad can read.
fn readable_image(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(format!("{} isn't an absolute path", path.display()));
    }
    let file = std::fs::File::open(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    match file.metadata() {
        Ok(metadata) if metadata.is_file() => Ok(path),
        Ok(_) => Err(format!("{} isn't a file", path.display())),
        Err(e) => Err(format!("can't read {}: {e}", path.display())),
    }
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

/// What awww shows, as far as novad knows; forgotten when the daemon exits.
#[derive(Default)]
struct Shown {
    /// By output, when the outputs are known.
    outputs: BTreeMap<String, PathBuf>,
    /// The whole choice, when they aren't.
    unfollowed: Option<Choice>,
}

impl Shared {
    fn run(self: &Arc<Self>, path_var: &str, logger: &Arc<Logger>) {
        let mut shown = Shown::default();
        let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
        let mut started = Instant::now();
        loop {
            let mut state = lock(&self.state);
            state = self
                .wake
                .wait_while(state, |state| !state.stopped && !state.changed && !state.exited)
                .unwrap_or_else(|e| e.into_inner());
            if state.stopped {
                return;
            }
            let crashed = std::mem::take(&mut state.exited);
            if crashed {
                shown = Shown::default();
            }
            for name in std::mem::take(&mut state.gone) {
                shown.outputs.remove(&name);
            }
            state.changed = false;
            let Some(wanted) = state.wanted() else {
                if let Some(pid) = state.daemon {
                    logger.info(LOG_SOURCE, &format!("no wallpaper to show, stopping {DAEMON}"));
                    terminate(pid);
                }
                continue;
            };
            if state.daemon.is_none() {
                if crashed {
                    if started.elapsed() >= RAN_FINE {
                        retry.reset();
                    }
                    let wait = retry.next();
                    logger.warn(LOG_SOURCE, &format!("{DAEMON} ended, starting it again in {}s", wait.as_secs()));
                    let (mut waited, _) = self
                        .wake
                        .wait_timeout_while(state, wait, |state| !state.stopped)
                        .unwrap_or_else(|e| e.into_inner());
                    // Looked at afresh: anything may have changed meanwhile.
                    waited.changed = true;
                    continue;
                }
                started = Instant::now();
                // Spawned under the lock, so `stop` sees the pid or stops
                // the thread before the spawn.
                if let Err(e) = self.spawn_daemon(&mut state, path_var, logger) {
                    logger.warn(LOG_SOURCE, &format!("can't start {DAEMON}: {e}"));
                    state.exited = true;
                    continue;
                }
                drop(state);
                if let Err(e) = self.wait_ready(path_var) {
                    logger.warn(LOG_SOURCE, &format!("{DAEMON} didn't start answering: {e}"));
                    // Started again, after a wait, once it's gone.
                    if let Some(pid) = lock(&self.state).daemon {
                        terminate(pid);
                    }
                    continue;
                }
            } else {
                drop(state);
            }
            let outputs = lock(&self.state).outputs.clone();
            show(&wanted, &outputs, &mut shown, path_var, logger);
        }
    }

    /// Starts the daemon and a thread that waits for it to exit.
    fn spawn_daemon(self: &Arc<Self>, state: &mut State, path_var: &str, logger: &Arc<Logger>) -> std::io::Result<()> {
        let mut command = Command::new(DAEMON);
        command
            .args(["--namespace", NAMESPACE, "--no-cache", "--quiet"])
            .env("PATH", path_var)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        // As the UI does. This thread, the spawning one, runs until `stop`.
        child::die_with_novad(&mut command, libc::SIGTERM);
        let mut daemon = command.spawn()?;
        let pid = daemon.id();
        state.daemon = Some(pid);
        logger.info(LOG_SOURCE, &format!("started {DAEMON} (pid {pid})"));
        let errors = child::drain(daemon.stderr.take());
        let shared = Arc::clone(self);
        let logger = Arc::clone(logger);
        std::thread::spawn(move || {
            if let Err(e) = child::wait_for_exit(pid) {
                logger.warn(LOG_SOURCE, &format!("couldn't wait for {DAEMON} without reaping it: {e}"));
            }
            let mut state = lock(&shared.state);
            // Forgotten before the reap, so nothing signals a pid the
            // kernel may have handed out again.
            state.daemon = None;
            let status = daemon.wait().map_or_else(|e| e.to_string(), |status| status.to_string());
            let said = errors.text();
            logger.info(LOG_SOURCE, &format!("{DAEMON} exited ({status}) {}", said.trim()));
            state.exited = true;
            shared.wake.notify_all();
        });
        Ok(())
    }

    /// Waits until the daemon answers, or exits.
    fn wait_ready(&self, path_var: &str) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let mut query = awww(path_var);
            query.args(["query", "--namespace", NAMESPACE]);
            let answer = child::output_within(query, QUERY_TIMEOUT);
            if answer.as_ref().is_ok_and(|output| output.status.success()) {
                return Ok(());
            }
            if lock(&self.state).daemon.is_none() {
                return Err("it exited".into());
            }
            if Instant::now() >= deadline {
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
fn show(wanted: &Choice, outputs: &Outputs, shown: &mut Shown, path_var: &str, logger: &Logger) {
    match outputs {
        Outputs::Unknown => {}
        Outputs::Known(names) => {
            // One fade for every output that changes to the same image.
            let mut changes: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
            for (name, path) in wanted.per_output(names) {
                if shown.outputs.get(&name) != Some(&path) {
                    changes.entry(path).or_default().push(name);
                }
            }
            for (path, names) in changes {
                match send_image(&path, Some(&names), path_var) {
                    Ok(()) => shown.outputs.extend(names.into_iter().map(|name| (name, path.clone()))),
                    Err(e) => logger.warn(LOG_SOURCE, &format!("can't show {}: {e}", path.display())),
                }
            }
        }
        Outputs::Unfollowed => {
            if shown.unfollowed.as_ref() == Some(wanted) {
                return;
            }
            let every = wanted.path.iter().map(|path| (path, None));
            let own = wanted.outputs.iter().map(|(name, path)| (path, Some(vec![name.clone()])));
            let mut all_shown = true;
            for (path, names) in every.chain(own) {
                if let Err(e) = send_image(path, names.as_deref(), path_var) {
                    logger.warn(LOG_SOURCE, &format!("can't show {}: {e}", path.display()));
                    all_shown = false;
                }
            }
            shown.unfollowed = all_shown.then(|| wanted.clone());
        }
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

fn terminate(pid: u32) {
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(pid.cast_signed(), libc::SIGTERM) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Hub;
    use nova_protocol::LogLevel;
    use nova_test_support::{TempDir, eventually, wait_until};

    const WAIT: Duration = Duration::from_secs(10);

    struct Fixture {
        dir: TempDir,
        logger: Arc<Logger>,
    }

    impl Fixture {
        /// Stand-ins for awww in `<dir>/bin`: the daemon writes its pid to
        /// `<dir>/runs` each time it starts, then sleeps; the client writes
        /// its arguments to `<dir>/awww` and succeeds. Two images in `<dir>`.
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
                &format!("echo \"$*\" >> '{}'", dir.join("awww").display()),
            );
            for image in ["dusk.jpg", "tide.jpg"] {
                std::fs::write(dir.join(image), "an image").unwrap();
            }
            let logger =
                Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap());
            Self { dir, logger }
        }

        fn start(&self) -> Arc<Wallpaper> {
            let path = format!("{}:/run/current-system/sw/bin:/usr/bin:/bin", self.dir.join("bin").display());
            Wallpaper::start(&self.dir.join("state"), path, true, Arc::clone(&self.logger))
        }

        fn image(&self, name: &str) -> String {
            self.dir.join(name).display().to_string()
        }

        /// `[wallpaper] path` set to the image `name`.
        fn config(&self, name: &str) -> WallpaperConfig {
            WallpaperConfig { path: Some(self.image(name)), outputs: BTreeMap::new() }
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
        wallpaper.configure(true, &WallpaperConfig::default());
        std::thread::sleep(Duration::from_millis(300));
        assert!(!fixture.dir.join("runs").exists(), "a wallpaper tool of the user's keeps the screen");
        wallpaper.set(&fixture.image("dusk.jpg"), None).unwrap();
        fixture.runs(1);
        assert_eq!(fixture.images(1), [("DP-1,DP-2".into(), fixture.image("dusk.jpg"))]);
        let line = std::fs::read_to_string(fixture.dir.join("awww")).unwrap();
        assert!(line.contains("--namespace nova") && line.contains("--transition-type fade"), "{line}");
        wallpaper.stop();
    }

    #[test]
    fn only_outputs_that_change_get_an_image() {
        let fixture = Fixture::new("changes");
        let wallpaper = fixture.start();
        wallpaper.configure(true, &fixture.config("dusk.jpg"));
        std::thread::sleep(Duration::from_millis(300));
        assert!(fixture.sent().is_empty(), "nothing before the outputs are known");
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        let dusk = fixture.image("dusk.jpg");
        assert_eq!(fixture.images(1), [("DP-1,DP-2".into(), dusk.clone())]);
        wallpaper.set(&fixture.image("tide.jpg"), Some("DP-2")).unwrap();
        assert_eq!(fixture.images(2)[1], ("DP-2".into(), fixture.image("tide.jpg")));
        // Plugged in: only the new output gets one.
        wallpaper.outputs(outputs(&["DP-1", "DP-2", "HDMI-A-1"]));
        assert_eq!(fixture.images(3)[2], ("HDMI-A-1".into(), dusk.clone()));
        // Unplugged and back: it gets its image again.
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.outputs(outputs(&["DP-1", "DP-2", "HDMI-A-1"]));
        assert_eq!(fixture.images(4)[3], ("HDMI-A-1".into(), dusk));
        assert_eq!(wallpaper.current().outputs["DP-2"], fixture.image("tide.jpg"));
        assert_eq!(fixture.runs(1).len(), 1);
        wallpaper.stop();
    }

    #[test]
    fn the_choice_lasts_until_the_config_changes() {
        let fixture = Fixture::new("saved");
        let config = fixture.config("dusk.jpg");
        let first = fixture.start();
        first.configure(true, &config);
        first.set(&fixture.image("tide.jpg"), None).unwrap();
        first.stop();
        // Started again with the same config: the choice stays.
        let again = fixture.start();
        again.configure(true, &config);
        assert_eq!(again.current().path, Some(fixture.image("tide.jpg")));
        // The config changes: it wins, and the choice is gone.
        let edited = WallpaperConfig { outputs: [("DP-2".into(), fixture.image("tide.jpg"))].into(), ..config };
        again.configure(true, &edited);
        assert_eq!(again.current().path, Some(fixture.image("dusk.jpg")));
        assert!(!fixture.dir.join("state").join(STATE_FILE).exists());
        again.stop();
        // So it's gone after a restart too.
        let last = fixture.start();
        last.configure(true, &edited);
        assert_eq!(last.current().path, Some(fixture.image("dusk.jpg")));
        last.stop();
    }

    #[test]
    fn the_config_changing_while_novad_is_off_wins() {
        let fixture = Fixture::new("offline-edit");
        let first = fixture.start();
        first.configure(true, &fixture.config("dusk.jpg"));
        first.set(&fixture.image("tide.jpg"), None).unwrap();
        first.stop();
        let again = fixture.start();
        again.configure(true, &WallpaperConfig::default());
        assert_eq!(again.current(), Wallpapers::default());
        again.stop();
    }

    #[test]
    fn a_crashed_daemon_starts_again_and_gets_its_images_back() {
        let fixture = Fixture::new("crash");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1"]));
        wallpaper.configure(true, &fixture.config("dusk.jpg"));
        let first = fixture.runs(1)[0];
        fixture.images(1);
        // SAFETY: kill only sends a signal.
        unsafe { libc::kill(first, libc::SIGKILL) };
        let second = fixture.runs(2)[1];
        assert_ne!(first, second);
        assert_eq!(fixture.images(2)[1], ("DP-1".into(), fixture.image("dusk.jpg")));
        wallpaper.stop();
        wait_until(WAIT, "stopping to end it", || !alive(second));
    }

    #[test]
    fn the_daemon_stops_with_the_feature() {
        let fixture = Fixture::new("off");
        let wallpaper = fixture.start();
        wallpaper.configure(true, &fixture.config("dusk.jpg"));
        let pid = fixture.runs(1)[0];
        wallpaper.configure(false, &fixture.config("dusk.jpg"));
        wait_until(WAIT, "the daemon to stop", || !alive(pid));
        assert!(wallpaper.set(&fixture.image("tide.jpg"), None).unwrap_err().contains("isn't ready"));
        std::thread::sleep(FIRST_RETRY + Duration::from_millis(200));
        assert_eq!(fixture.runs(1).len(), 1, "not started again while off");
        wallpaper.stop();
    }

    #[test]
    fn a_bad_choice_is_refused_and_changes_nothing() {
        let fixture = Fixture::new("refused");
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.configure(true, &fixture.config("dusk.jpg"));
        assert!(wallpaper.set("dusk.jpg", None).unwrap_err().contains("absolute"));
        assert!(wallpaper.set(&fixture.image("missing.jpg"), None).unwrap_err().contains("can't read"));
        assert!(wallpaper.set(&fixture.dir.display().to_string(), None).unwrap_err().contains("isn't a file"));
        let unknown = wallpaper.set(&fixture.image("tide.jpg"), Some("HDMI-A-1")).unwrap_err();
        assert!(unknown.contains("DP-1, DP-2"), "{unknown}");
        assert!(wallpaper.set(&fixture.image("tide.jpg"), Some("DP-1,DP-2")).is_err());
        assert_eq!(wallpaper.current().path, Some(fixture.image("dusk.jpg")));
        assert!(!fixture.dir.join("state").join(STATE_FILE).exists());
        wallpaper.stop();
    }
}
