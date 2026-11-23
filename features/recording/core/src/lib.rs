//! Screen recording (feature `recording`): a whole output, or a region of
//! the layout, into the recording folder, with the desktop's sound and the
//! mic when asked.
//!
//! `nova record` asks the UI to open its region picker (`pick_recording`);
//! the UI counts down and sends `start_recording`. The same key stops the
//! recording, as does the bar's recording pill (`stop_recording`).
//!
//! gpu-screen-recorder records when it's installed (it encodes on the GPU,
//! or the CPU when the GPU can't, and can pause), else wf-recorder. Either
//! writes a hidden file in the recording folder, which is moved to its name
//! once the recorder has
//! finished it, so a name only ever shows a whole video. The recorder is
//! novad's child: SIGINT tells it to finish the file, and it gets that too
//! when novad goes away, so a crash doesn't leave it recording.
//!
//! Its messages are `nova-recording-api`'s, and its `[recording]` settings
//! are [`RecordingConfig`]. Saved files are named, placed and acted on as
//! screenshots are, through the [`SavedFiles`] port the app fills.

mod mp4;

use nova_base::lock;
use nova_base::reply_slot::Slot;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_process::child;
use nova_recording_api::{ActiveRecording, Event, LayoutRect, RecordAudio, SavedRecording, ScreenshotAction};
use serde::Deserialize;
use std::collections::VecDeque;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "recording";
/// This feature's name in the log, which [`SavedFiles::act_on`] logs under.
pub const LOG_SOURCE: &str = "recording";

/// Records on the GPU (NVENC, VA-API) and can pause.
const GPU_SCREEN_RECORDER: &str = "gpu-screen-recorder";
/// Records through wlr-screencopy, without pausing.
const WF_RECORDER: &str = "wf-recorder";
/// How long a recorder that fails at once (a missing capture permission, a
/// bad output) has to say so before the start counts as a success.
const START_GRACE: Duration = Duration::from_millis(800);
/// How long a stopped recorder may take to finish the file before it's
/// killed.
const FINISH_TIMEOUT: Duration = if cfg!(test) { Duration::from_secs(1) } else { Duration::from_secs(10) };
/// What starts the line saying why, in either recorder's errors
/// (`gsr error: …`, `wf-recorder: error: …`); the earlier lines are warnings
/// and progress.
const ERROR_MARK: &str = "error: ";
/// Frames per second.
const FPS: &str = "60";
/// gpu-screen-recorder encodes on the CPU when the GPU can't (a driver
/// whose NVENC API is older than its FFmpeg wants, or no hardware encoder),
/// instead of exiting before it records anything.
const GSR_CPU_FALLBACK: [&str; 2] = ["-fallback-cpu-encoding", "yes"];
/// Saved recordings the card's buttons can still act on.
const KEEP_SAVED: usize = 32;
/// A saved recording's name, from the local time it started.
const NAME_PREFIX: &str = "Recording ";
/// The container both recorders pick from the file's extension.
const EXTENSION: &str = "mp4";
/// gpu-screen-recorder's names for the default output's sound and the
/// default input; `|` mixes sources into one track.
const GSR_DESKTOP: &str = "default_output";
const GSR_MIC: &str = "default_input";
/// PulseAudio's (and pipewire-pulse's) name for the default output's
/// monitor, for wf-recorder; without a name it records the default input.
const WF_DESKTOP: &str = "@DEFAULT_MONITOR@";
/// The wf-recorder audio backend those names and that default are
/// PulseAudio's for: its PipeWire one records the desktop by default.
const WF_AUDIO_BACKEND: &str = "pulse";

/// `[recording]` in config.toml.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RecordingConfig {
    /// Where recordings are saved: an absolute path, or one starting with
    /// `~/`. By default `Recordings` in the user's videos folder.
    #[serde(default)]
    pub folder: Option<String>,
}

impl RecordingConfig {
    /// The folder recordings go in (see [`nova_settings::folder`]).
    pub fn folder(&self) -> Result<PathBuf, String> {
        nova_settings::folder("recording", self.folder.as_deref(), || {
            nova_paths::user_dir("VIDEOS", "Videos").join(DEFAULT_FOLDER)
        })
    }
}

/// The recording folder's name inside the videos folder.
const DEFAULT_FOLDER: &str = "Recordings";

/// How saved files are named, placed and acted on: the screenshot
/// feature's helpers, a port the app fills so this feature doesn't use the
/// screenshot feature's code.
pub trait SavedFiles: Send + Sync + 'static {
    /// The local time, for a saved file's name.
    fn time_stamp(&self) -> String;
    /// Moves the finished `temporary` file to `<stem>.<extension>`, or
    /// `<stem> (2).<extension>` and so on: never over an existing file.
    fn place(&self, temporary: &Path, stem: &Path, extension: &str) -> Result<PathBuf, String>;
    /// Opens a saved file, its folder, or deletes it, logging under
    /// [`LOG_SOURCE`].
    fn act_on(&self, path: &Path, action: ScreenshotAction, path_var: &str, logger: &Arc<Logger>)
    -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recorder {
    Gpu,
    Wf,
}

pub struct Recordings {
    /// Where the recorders are looked for.
    path_var: String,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    files: Arc<dyn SavedFiles>,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// The feature is on: starts are refused while it isn't.
    allowed: bool,
    active: Option<Active>,
    /// The latest saved recordings, oldest first.
    saved: VecDeque<SavedRecording>,
    next_id: u64,
    /// Tells a finished recorder apart from a newer one.
    runs: u64,
}

struct Active {
    run: u64,
    pid: libc::pid_t,
    recorder: Recorder,
    output: String,
    region: Option<LayoutRect>,
    audio: RecordAudio,
    /// The file the recorder writes, and the name it gets once finished.
    file: PathBuf,
    stem: PathBuf,
    started: Instant,
    /// Since when it's paused, and for how long it was before.
    paused_since: Option<Instant>,
    paused_for: Duration,
    /// Asked to finish.
    stopping: bool,
    /// Resolved by its keeper once the recorder exits.
    exited: Arc<Slot<()>>,
}

impl Active {
    fn elapsed(&self) -> Duration {
        let paused = self.paused_for + self.paused_since.map_or(Duration::ZERO, |since| since.elapsed());
        self.started.elapsed().saturating_sub(paused)
    }

    fn event(&self) -> ActiveRecording {
        ActiveRecording {
            output: self.output.clone(),
            region: self.region,
            audio: self.audio,
            elapsed_ms: millis(self.elapsed()),
            paused: self.paused_since.is_some(),
            can_pause: self.recorder == Recorder::Gpu,
        }
    }

    /// Sends the recorder `signal`. Only while it's `active`: its keeper
    /// clears that under the same lock before reaping it, so the pid is
    /// still the recorder's.
    fn signal(&self, signal: libc::c_int) {
        // SAFETY: kill takes no pointers.
        unsafe { libc::kill(self.pid, signal) };
    }
}

impl Recordings {
    pub fn new(path_var: String, hub: Arc<Hub>, logger: Arc<Logger>, files: Arc<dyn SavedFiles>) -> Self {
        Self { path_var, hub, logger, files, state: Arc::default() }
    }

    /// `nova record`: stops the recording, or asks the UI to open the
    /// picker for one.
    pub fn record(&self) -> Result<(), String> {
        if lock(&self.state).active.is_some() {
            return self.stop();
        }
        match self.hub.publish(&Event::PickRecording) {
            0 => Err("the UI isn't connected to novad, so there's nothing to pick what to record with".into()),
            _ => Ok(()),
        }
    }

    /// Starts recording `output`, or `region` of the layout, into `folder`.
    /// Answers once the recorder has had [`START_GRACE`] to fail.
    pub fn start(
        &self,
        output: &str,
        region: Option<LayoutRect>,
        audio: RecordAudio,
        folder: &Path,
    ) -> Result<(), String> {
        let mut state = lock(&self.state);
        if !state.allowed {
            return Err(nova_modules::not_ready(FEATURE_ID));
        }
        if state.active.is_some() {
            return Err("already recording".into());
        }
        if region.is_some_and(|region| region.width == 0 || region.height == 0) {
            return Err("the region is empty".into());
        }
        let (recorder, program) = self.recorder()?;
        std::fs::create_dir_all(folder).map_err(|e| format!("can't make {}: {e}", folder.display()))?;
        let stamp = self.files.time_stamp();
        let stem = folder.join(format!("{NAME_PREFIX}{stamp}"));
        let run = state.runs + 1;
        // The run keeps two recordings started within a second apart while
        // the first is still being placed.
        let file = folder.join(format!(".{NAME_PREFIX}{stamp}.{}-{run}.{EXTENSION}", std::process::id()));
        let args = match recorder {
            Recorder::Gpu => gsr_args(output, region, audio, &file),
            Recorder::Wf => {
                if audio.desktop && audio.mic {
                    self.logger.warn(LOG_SOURCE, "wf-recorder takes one sound source: recording the desktop only");
                }
                wf_args(output, region, audio, &file)
            }
        };
        let mut command = Command::new(&program);
        command
            .args(&args)
            .env("PATH", &self.path_var)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        // Finishes the file and exits when novad goes away. The thread that
        // spawns it is its keeper, which lives as long as it does.
        child::die_with_novad(&mut command, libc::SIGINT);
        // Its own process group: a Ctrl+C on `nova daemon` in a terminal
        // reaches novad, whose exit stops it the same way.
        command.process_group(0);
        let (started, start_result) = mpsc::channel();
        let exited = Arc::new(Slot::default());
        let keeper = Keeper {
            exited: Arc::clone(&exited),
            state: Arc::clone(&self.state),
            hub: Arc::clone(&self.hub),
            logger: Arc::clone(&self.logger),
            files: Arc::clone(&self.files),
            run,
        };
        std::thread::spawn(move || keeper.keep(command, recorder, started));
        // The keeper's answer; `state` stays locked meanwhile, so the
        // recorder can't be finished before it's active.
        let pid = start_result.recv().unwrap_or_else(|_| Err("the recorder's keeper went away".into())).inspect_err(
            |_| {
                let _ = std::fs::remove_file(&file);
            },
        )?;
        state.runs = run;
        let active = Active {
            run,
            pid,
            recorder,
            output: output.into(),
            region,
            audio,
            file,
            stem,
            started: Instant::now() - START_GRACE,
            paused_since: None,
            paused_for: Duration::ZERO,
            stopping: false,
            exited,
        };
        self.logger.info(
            LOG_SOURCE,
            &format!("recording {} with {} into {}", what(output, region), recorder.name(), folder.display()),
        );
        self.hub.publish(&Event::Recording(Some(active.event())));
        state.active = Some(active);
        Ok(())
    }

    /// Asks the recorder to finish the file; it's saved once it has. One
    /// that doesn't within [`FINISH_TIMEOUT`] is killed.
    pub fn stop(&self) -> Result<(), String> {
        self.stop_in(&mut lock(&self.state))
    }

    /// Whether the feature is on, as settings say. Turning it off stops a
    /// recording: the UI drops the pill that shows and stops it. Under the
    /// same lock as `start`, so a start already asked for can't begin after.
    pub fn allow(&self, allowed: bool) {
        let mut state = lock(&self.state);
        state.allowed = allowed;
        if !allowed && state.active.is_some() {
            let _ = self.stop_in(&mut state);
        }
    }

    fn stop_in(&self, state: &mut State) -> Result<(), String> {
        let active = state.active.as_mut().ok_or("nothing is being recorded")?;
        if active.stopping {
            return Ok(());
        }
        active.stopping = true;
        // A paused gpu-screen-recorder finishes all the same.
        active.signal(libc::SIGINT);
        let run = active.run;
        let exited = Arc::clone(&active.exited);
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            if exited.wait_for(FINISH_TIMEOUT).is_some() {
                return;
            }
            if let Some(active) = lock(&state).active.as_ref().filter(|active| active.run == run) {
                active.signal(libc::SIGKILL);
            }
        });
        Ok(())
    }

    /// Pauses or resumes the recording, when the recorder can.
    pub fn pause(&self, paused: bool) -> Result<(), String> {
        let mut state = lock(&self.state);
        let active = state.active.as_mut().ok_or("nothing is being recorded")?;
        if active.recorder != Recorder::Gpu {
            return Err(format!("{} can't pause", active.recorder.name()));
        }
        if active.paused_since.is_some() == paused || active.stopping {
            return Ok(());
        }
        // gpu-screen-recorder pauses and resumes on SIGUSR2.
        active.signal(libc::SIGUSR2);
        match active.paused_since.take() {
            Some(since) => active.paused_for += since.elapsed(),
            None => active.paused_since = Some(Instant::now()),
        }
        self.hub.publish(&Event::Recording(Some(active.event())));
        Ok(())
    }

    /// Sends `client` what's being recorded.
    pub fn send_state(&self, client: ClientId) {
        // Sent under the lock, so it can't overtake a newer state published
        // meanwhile.
        let state = lock(&self.state);
        self.hub.send_to(client, &Event::Recording(state.active.as_ref().map(Active::event)));
    }

    /// Saves what earlier novads left in `folder`: one that quit, was
    /// stopped or crashed mid-recording took the keeper with it, and its
    /// recorder finished the file (the parent-death SIGINT) under its hidden
    /// name. A recorder still finishing one keeps writing to it once moved.
    pub fn recover(&self, folder: &Path) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        let own = std::process::id() as libc::pid_t;
        for path in entries.flatten().map(|entry| entry.path()) {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(rest) = name.strip_prefix(&format!(".{NAME_PREFIX}")) else {
                continue;
            };
            let Some((stamp, pid)) = rest.strip_suffix(&format!(".{EXTENSION}")).and_then(|rest| rest.rsplit_once('.'))
            else {
                continue;
            };
            let pid = pid.split_once('-').map_or(pid, |(pid, _run)| pid);
            let Ok(pid) = pid.parse::<libc::pid_t>() else {
                continue;
            };
            // Its own, or another novad's that's still running (one on
            // another socket can share the folder).
            if pid == own || running(pid) {
                continue;
            }
            if std::fs::metadata(&path).is_ok_and(|meta| meta.len() == 0)
                || mp4::contents(&path) == mp4::Contents::NoFrames
            {
                let _ = std::fs::remove_file(&path);
                self.logger.info(LOG_SOURCE, &format!("{} from an earlier run held no frames", path.display()));
                continue;
            }
            match self.files.place(&path, &folder.join(format!("{NAME_PREFIX}{stamp}")), EXTENSION) {
                Ok(saved) => self.logger.info(LOG_SOURCE, &format!("saved {} from an earlier run", saved.display())),
                Err(e) => self.logger.warn(LOG_SOURCE, &format!("{} stays hidden: {e}", path.display())),
            }
        }
    }

    /// Opens nova.log (the card's Open log, after a recording didn't start).
    pub fn open_log(&self) -> Result<(), String> {
        nova_apps::open(&nova_paths::log_file(&self.logger.dir()), &self.path_var, &self.logger)
    }

    /// Opens a saved recording, its folder, or deletes it.
    pub fn act(&self, id: u64, action: ScreenshotAction) -> Result<(), String> {
        let mut state = lock(&self.state);
        let index = state
            .saved
            .iter()
            .position(|saved| saved.id == id)
            .ok_or_else(|| format!("no recording {id} (it's from before novad started, or deleted)"))?;
        let path = PathBuf::from(&state.saved[index].path);
        self.files.act_on(&path, action, &self.path_var, &self.logger)?;
        if action == ScreenshotAction::Delete {
            state.saved.remove(index);
        }
        Ok(())
    }

    /// gpu-screen-recorder when it's installed, else wf-recorder.
    fn recorder(&self) -> Result<(Recorder, PathBuf), String> {
        [Recorder::Gpu, Recorder::Wf]
            .into_iter()
            .find_map(|recorder| {
                nova_paths::find_program(recorder.name(), &self.path_var).map(|program| (recorder, program))
            })
            .ok_or_else(|| format!("neither {GPU_SCREEN_RECORDER} nor {WF_RECORDER} is installed"))
    }
}

impl Recorder {
    fn name(self) -> &'static str {
        match self {
            Recorder::Gpu => GPU_SCREEN_RECORDER,
            Recorder::Wf => WF_RECORDER,
        }
    }
}

/// The thread a recorder is spawned from and lives with: the kernel sends
/// the recorder PR_SET_PDEATHSIG's signal when that thread ends, not only
/// when novad does. It says whether the recorder started, then waits for it
/// to exit and finishes the recording.
struct Keeper {
    /// Resolved once the recorder exits.
    exited: Arc<Slot<()>>,
    state: Arc<Mutex<State>>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    files: Arc<dyn SavedFiles>,
    run: u64,
}

impl Keeper {
    fn keep(self, mut command: Command, recorder: Recorder, started: mpsc::Sender<Result<libc::pid_t, String>>) {
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = started.send(Err(format!("can't run {}: {e}", recorder.name())));
                return;
            }
        };
        let errors = child::drain(child.stderr.take());
        match child::wait_within(&mut child, START_GRACE) {
            Ok(None) => {}
            Ok(Some(status)) => {
                let message = errors.text();
                self.logger
                    .warn(LOG_SOURCE, &format!("{} didn't start ({status}): {}", recorder.name(), message.trim()));
                let _ = started.send(Err(why_not_started(recorder, status, &message)));
                return;
            }
            Err(e) => {
                child::kill_and_reap(&mut child);
                let _ = started.send(Err(format!("lost track of the recorder: {e}")));
                return;
            }
        }
        let pid = child.id() as libc::pid_t;
        if started.send(Ok(pid)).is_err() {
            // Nothing will stop it: it's not active.
            let _ = child.kill();
        }
        // Not reaped yet, so the pid stays the recorder's until it's no
        // longer active and nothing signals it any more.
        // Only fails for a pid that isn't a child, which this one is.
        let _ = child::wait_for_exit(pid.cast_unsigned());
        self.exited.resolve(());
        let active = {
            let mut state = lock(&self.state);
            let active = state.active.take_if(|active| active.run == self.run);
            if active.is_some() {
                self.hub.publish(&Event::Recording(None));
            }
            active.map(|active| (active.elapsed(), active))
        };
        let status = child.wait();
        let message = errors.text();
        if let Some((duration, active)) = active {
            self.finish(active, duration, status, message.trim());
        }
    }

    /// A recording's recorder exited: saves what it wrote.
    fn finish(
        &self,
        active: Active,
        duration: Duration,
        status: std::io::Result<std::process::ExitStatus>,
        message: &str,
    ) {
        let finished = matches!(&status, Ok(status) if status.success());
        if !finished && !active.stopping {
            let status = status.map_or_else(|e| e.to_string(), |status| status.to_string());
            self.logger
                .error(LOG_SOURCE, &format!("{} stopped by itself ({status}): {message}", active.recorder.name()));
        }
        // Something was recorded: kept even when the recorder didn't finish
        // cleanly, as players read most of a cut-off file. A header with no
        // frame after it (a recorder stalled waiting for sound, then
        // killed) isn't a recording.
        let written = std::fs::metadata(&active.file).is_ok_and(|meta| meta.len() > 0);
        if !written || mp4::contents(&active.file) == mp4::Contents::NoFrames {
            let _ = std::fs::remove_file(&active.file);
            let reason = if finished {
                format!("{} recorded no frames", active.recorder.name())
            } else {
                format!("{} stopped before it recorded a frame", active.recorder.name())
            };
            self.logger.warn(LOG_SOURCE, &format!("nothing was recorded: {reason}"));
            // Turning the feature off stops a recording too: no card then.
            if lock(&self.state).allowed {
                self.hub.publish(&Event::RecordingFailed { reason });
            }
            return;
        }
        match self.files.place(&active.file, &active.stem, EXTENSION) {
            Ok(path) => {
                self.logger.info(LOG_SOURCE, &format!("saved {} ({} s)", path.display(), duration.as_secs()));
                let mut state = lock(&self.state);
                state.next_id += 1;
                let saved = SavedRecording {
                    id: state.next_id,
                    path: path.display().to_string(),
                    duration_ms: millis(duration),
                };
                if state.saved.len() == KEEP_SAVED {
                    state.saved.pop_front();
                }
                state.saved.push_back(saved.clone());
                self.hub.publish(&Event::RecordingSaved(saved));
            }
            Err(e) => self.logger.error(LOG_SOURCE, &format!("the recording stays at {}: {e}", active.file.display())),
        }
    }
}

/// What the recording card says when the recorder quit at once: its last
/// error line, without the recorder's own prefix. All it said is in the log.
fn why_not_started(recorder: Recorder, status: std::process::ExitStatus, errors: &str) -> String {
    let lines: Vec<&str> = errors.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let line = lines.iter().rev().find(|line| line.to_ascii_lowercase().contains(ERROR_MARK)).or(lines.last());
    match line {
        Some(line) => {
            let reason = line.to_ascii_lowercase().find(ERROR_MARK).map_or(*line, |at| &line[at + ERROR_MARK.len()..]);
            format!("{} didn't start: {}", recorder.name(), reason.trim())
        }
        None => format!("{} didn't start ({status})", recorder.name()),
    }
}

/// Whether a process `pid` exists, someone else's included.
fn running(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
}

/// What's recorded, for the log.
fn what(output: &str, region: Option<LayoutRect>) -> String {
    match region {
        Some(r) => format!("{}x{} at {},{} on {output}", r.width, r.height, r.x, r.y),
        None => output.into(),
    }
}

fn gsr_args(output: &str, region: Option<LayoutRect>, audio: RecordAudio, file: &Path) -> Vec<String> {
    let mut args: Vec<String> = match region {
        Some(r) => {
            vec!["-w".into(), "region".into(), "-region".into(), format!("{}x{}+{}+{}", r.width, r.height, r.x, r.y)]
        }
        None => vec!["-w".into(), output.into()],
    };
    args.extend(["-f".into(), FPS.into()]);
    args.extend(GSR_CPU_FALLBACK.map(String::from));
    let sources: Vec<&str> = [(audio.desktop, GSR_DESKTOP), (audio.mic, GSR_MIC)]
        .into_iter()
        .filter_map(|(on, source)| on.then_some(source))
        .collect();
    if !sources.is_empty() {
        args.extend(["-a".into(), sources.join("|")]);
    }
    args.extend(["-o".into(), file.display().to_string()]);
    args
}

fn wf_args(output: &str, region: Option<LayoutRect>, audio: RecordAudio, file: &Path) -> Vec<String> {
    // -y: the file is new, but wf-recorder would ask on a terminal otherwise.
    let mut args: Vec<String> = vec!["-y".into()];
    match region {
        Some(r) => args.extend(["-g".into(), format!("{},{} {}x{}", r.x, r.y, r.width, r.height)]),
        None => args.extend(["-o".into(), output.into()]),
    }
    args.extend(["-r".into(), FPS.into()]);
    if audio.desktop {
        args.push(format!("--audio={WF_DESKTOP}"));
    } else if audio.mic {
        args.push("--audio".into());
    }
    if audio.desktop || audio.mic {
        args.push(format!("--audio-backend={WF_AUDIO_BACKEND}"));
    }
    args.extend(["-f".into(), file.display().to_string()]);
    args
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
