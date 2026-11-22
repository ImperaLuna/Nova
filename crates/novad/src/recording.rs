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

use crate::hub::Hub;
use crate::logger::Logger;
use crate::{child, lock};
use nova_protocol::{ActiveRecording, Event, LayoutRect, RecordAudio, SavedRecording};
use nova_screenshot as screenshot;
use nova_screenshot_api::ScreenshotAction;
use std::collections::VecDeque;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "recording";
const LOG_SOURCE: &str = "recording";

/// Records on the GPU (NVENC, VA-API) and can pause.
const GPU_SCREEN_RECORDER: &str = "gpu-screen-recorder";
/// Records through wlr-screencopy, without pausing.
const WF_RECORDER: &str = "wf-recorder";
/// How long a recorder that fails at once (a missing capture permission, a
/// bad output) has to say so before the start counts as a success.
const START_GRACE: Duration = Duration::from_millis(800);
/// How long a stopped recorder may take to finish the file before it's
/// killed.
const FINISH_TIMEOUT: Duration = Duration::from_secs(10);
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
    pub fn new(path_var: String, hub: Arc<Hub>, logger: Arc<Logger>) -> Self {
        Self { path_var, hub, logger, state: Arc::default() }
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
            return Err(crate::features::not_ready(FEATURE_ID));
        }
        if state.active.is_some() {
            return Err("already recording".into());
        }
        if region.is_some_and(|region| region.width == 0 || region.height == 0) {
            return Err("the region is empty".into());
        }
        let (recorder, program) = self.recorder()?;
        std::fs::create_dir_all(folder).map_err(|e| format!("can't make {}: {e}", folder.display()))?;
        let stamp = screenshot::time_stamp();
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
        let keeper = Keeper {
            state: Arc::clone(&self.state),
            hub: Arc::clone(&self.hub),
            logger: Arc::clone(&self.logger),
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
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            std::thread::sleep(FINISH_TIMEOUT);
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
    pub fn send_state(&self, hub: &Hub, client: crate::hub::ClientId) {
        // Sent under the lock, so it can't overtake a newer state published
        // meanwhile.
        let state = lock(&self.state);
        hub.send_to(client, &Event::Recording(state.active.as_ref().map(Active::event)));
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
            if std::fs::metadata(&path).is_ok_and(|meta| meta.len() == 0) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            match screenshot::place(&path, &folder.join(format!("{NAME_PREFIX}{stamp}")), EXTENSION) {
                Ok(saved) => self.logger.info(LOG_SOURCE, &format!("saved {} from an earlier run", saved.display())),
                Err(e) => self.logger.warn(LOG_SOURCE, &format!("{} stays hidden: {e}", path.display())),
            }
        }
    }

    /// Opens nova.log (the card's Open log, after a recording didn't start).
    pub fn open_log(&self) -> Result<(), String> {
        crate::apps::open(&nova_paths::log_file(&self.logger.dir()), &self.path_var, &self.logger)
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
        screenshot::act_on(&path, action, &self.path_var, &self.logger, LOG_SOURCE)?;
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
    state: Arc<Mutex<State>>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
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
            finish(&self.state, &self.hub, &self.logger, active, duration, status, message.trim());
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

/// A recording's recorder exited: saves what it wrote.
fn finish(
    state: &Mutex<State>,
    hub: &Hub,
    logger: &Logger,
    active: Active,
    duration: Duration,
    status: std::io::Result<std::process::ExitStatus>,
    message: &str,
) {
    let finished = matches!(&status, Ok(status) if status.success());
    if !finished && !active.stopping {
        let status = status.map_or_else(|e| e.to_string(), |status| status.to_string());
        logger.error(LOG_SOURCE, &format!("{} stopped by itself ({status}): {message}", active.recorder.name()));
    }
    // Something was recorded: kept even when the recorder didn't finish
    // cleanly, as players read most of a cut-off file.
    let written = std::fs::metadata(&active.file).is_ok_and(|meta| meta.len() > 0);
    if !written {
        let _ = std::fs::remove_file(&active.file);
        logger.warn(LOG_SOURCE, "nothing was recorded");
        return;
    }
    match screenshot::place(&active.file, &active.stem, EXTENSION) {
        Ok(path) => {
            logger.info(LOG_SOURCE, &format!("saved {} ({} s)", path.display(), duration.as_secs()));
            let mut state = lock(state);
            state.next_id += 1;
            let saved =
                SavedRecording { id: state.next_id, path: path.display().to_string(), duration_ms: millis(duration) };
            if state.saved.len() == KEEP_SAVED {
                state.saved.pop_front();
            }
            state.saved.push_back(saved.clone());
            hub.publish(&Event::RecordingSaved(saved));
        }
        Err(e) => logger.error(LOG_SOURCE, &format!("the recording stays at {}: {e}", active.file.display())),
    }
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

/// Kept for novad's life, so a reload can't leave a recording running:
/// turning the feature off stops it.
impl crate::modules::Module for Recordings {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_protocol::topics::RECORDING);
    type Sources = ();
    type Config = crate::config::RecordingConfig;

    fn sources(_: &crate::modules::Env) {}

    fn config(config: &crate::config::Config) -> crate::config::RecordingConfig {
        config.recording.clone()
    }

    /// Saves what an earlier novad left in the folder, when the feature is
    /// ready. Allowed to record once told the settings (`reconfigure`).
    fn start(
        ctx: &crate::modules::Context,
        (): &(),
        config: &crate::config::RecordingConfig,
        features: &[nova_protocol::FeatureState],
    ) -> Arc<Self> {
        let recordings = Arc::new(Self::new(ctx.path_var.clone(), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger)));
        if crate::features::is_ready(features, FEATURE_ID)
            && let Ok(folder) = config.folder()
        {
            recordings.recover(&folder);
        }
        recordings
    }

    fn reconfigure(
        &self,
        _: &crate::config::RecordingConfig,
        _: &crate::config::RecordingConfig,
        features: &[nova_protocol::FeatureState],
    ) -> crate::modules::Reconfigure {
        self.allow(crate::features::is_ready(features, FEATURE_ID));
        crate::modules::Reconfigure::InPlace
    }

    fn send_state(&self, client: crate::hub::ClientId) {
        Self::send_state(self, &self.hub, client);
    }

    fn stop(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing};
    use nova_protocol::{LogLevel, topics};
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(5);

    /// Recordings with a fake recorder (it writes the file it's given and
    /// finishes it on SIGINT), and a UI subscribed to the recording topic.
    struct Fixture {
        dir: nova_test_support::TempDir,
        recordings: Recordings,
        ui: Receiver<Outgoing>,
        _ui_stream: UnixStream,
    }

    impl Fixture {
        /// `recorder` is the fake's name; `body` runs first, before it
        /// starts writing.
        fn new(name: &str, recorder: &str, body: &str) -> Self {
            let dir = nova_test_support::TempDir::new(&format!("recording-{name}"));
            let bin = dir.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let file_flag = if recorder == WF_RECORDER { "-f" } else { "-o" };
            let script = format!(
                "echo \"$@\" > {dir}/args\n{body}\n\
                 for arg; do [ \"$previous\" = {file_flag} ] && out=$arg; previous=$arg; done\n\
                 trap 'echo finished >> \"$out\"; exit 0' INT\n\
                 trap 'echo paused >> {dir}/signals' USR2\n\
                 echo frames > \"$out\"\n\
                 while :; do sleep 0.02; done",
                dir = dir.display()
            );
            nova_test_support::write_program(&bin, recorder, &script);
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let (ui_stream, other_end) = UnixStream::pair().unwrap();
            let (queue, ui) = ClientQueue::new(&other_end).unwrap();
            let id = hub.register(queue);
            hub.subscribe(id, topics::RECORDING);
            let path_var = nova_test_support::fake_path(&bin, &["sleep"]);
            let recordings = Recordings::new(path_var, hub, logger);
            recordings.allow(true);
            Self { dir, recordings, ui, _ui_stream: ui_stream }
        }

        fn folder(&self) -> PathBuf {
            self.dir.join("Videos/Recordings")
        }

        fn args(&self) -> String {
            std::fs::read_to_string(self.dir.join("args")).unwrap().trim().to_string()
        }

        fn start(&self, region: Option<LayoutRect>, audio: RecordAudio) -> Result<(), String> {
            self.recordings.start("DP-1", region, audio, &self.folder())
        }

        /// The next event, waiting for it.
        fn next_event(&self) -> serde_json::Value {
            crate::hub::next_event(&self.ui, WAIT)
        }

        fn files(&self) -> Vec<String> {
            let mut files: Vec<String> = std::fs::read_dir(self.folder())
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            files.sort();
            files
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.recordings.stop();
        }
    }

    const REGION: LayoutRect = LayoutRect { x: 2560, y: -40, width: 960, height: 540 };

    #[test]
    fn what_an_earlier_novad_left_is_saved_under_its_name() {
        let fixture = Fixture::new("recover", GPU_SCREEN_RECORDER, "");
        let folder = fixture.folder();
        std::fs::create_dir_all(&folder).unwrap();
        let left = |stamp: &str, pid: u32, frames: &str| {
            std::fs::write(folder.join(format!(".{NAME_PREFIX}{stamp}.{pid}-1.{EXTENSION}")), frames).unwrap();
        };
        let mut gone = Command::new("true").spawn().unwrap();
        gone.wait().unwrap();
        let mut other = Command::new("sleep").arg("5").spawn().unwrap();
        left("2026-10-25 18.00.00", gone.id(), "frames");
        left("2026-10-25 18.05.00", gone.id(), "");
        left("2026-10-25 18.10.00", std::process::id(), "frames");
        left("2026-10-25 18.15.00", other.id(), "frames");
        fixture.recordings.recover(&folder);
        let _ = other.kill();
        let _ = other.wait();
        assert_eq!(
            fixture.files(),
            [
                format!(".{NAME_PREFIX}2026-10-25 18.10.00.{}-1.{EXTENSION}", std::process::id()),
                format!(".{NAME_PREFIX}2026-10-25 18.15.00.{}-1.{EXTENSION}", other.id()),
                format!("{NAME_PREFIX}2026-10-25 18.00.00.{EXTENSION}"),
            ],
            "an empty one goes, and this novad's own and a running one's stay"
        );
    }

    #[test]
    fn turning_the_feature_off_stops_the_recording_and_refuses_new_ones() {
        let fixture = Fixture::new("off", GPU_SCREEN_RECORDER, "");
        fixture.start(None, RecordAudio::default()).unwrap();
        fixture.next_event();
        fixture.recordings.allow(false);
        assert_eq!(fixture.next_event()["data"], serde_json::Value::Null);
        assert_eq!(fixture.next_event()["event"], "recording_saved");
        assert!(fixture.start(None, RecordAudio::default()).unwrap_err().contains("isn't ready"));
    }

    #[test]
    fn the_recording_outlives_the_thread_that_started_it() {
        // Requests start recordings on threads of their own that end once
        // they've answered; PR_SET_PDEATHSIG fires when the spawning thread
        // does.
        let fixture = Fixture::new("thread", GPU_SCREEN_RECORDER, "");
        let (recordings, folder) = (&fixture.recordings, fixture.folder());
        std::thread::scope(|scope| {
            scope.spawn(|| recordings.start("DP-1", None, RecordAudio::default(), &folder).unwrap());
        });
        assert_eq!(fixture.next_event()["data"]["output"], "DP-1");
        std::thread::sleep(Duration::from_millis(300));
        assert!(lock(&fixture.recordings.state).active.is_some(), "the recorder stopped with its thread");
        let file = fixture.folder().join(&fixture.files()[0]);
        assert_eq!(std::fs::read_to_string(file).unwrap(), "frames\n", "it was told to finish");
    }

    #[test]
    fn a_stopped_recording_is_finished_then_saved_under_its_name() {
        let fixture = Fixture::new("saved", GPU_SCREEN_RECORDER, "");
        fixture.start(None, RecordAudio::default()).unwrap();
        assert_eq!(
            fixture.args(),
            format!(
                "-w DP-1 -f 60 -fallback-cpu-encoding yes -o {}",
                fixture.folder().join(fixture.files()[0].clone()).display()
            )
        );
        let started = fixture.next_event();
        assert_eq!(started["data"]["output"], "DP-1", "{started}");
        assert_eq!(started["data"]["can_pause"], true);
        assert_eq!(fixture.start(None, RecordAudio::default()).unwrap_err(), "already recording");

        fixture.recordings.stop().unwrap();
        assert_eq!(fixture.next_event()["data"], serde_json::Value::Null);
        let saved = fixture.next_event();
        assert_eq!(saved["event"], "recording_saved", "{saved}");
        let files = fixture.files();
        assert_eq!(files.len(), 1, "the hidden file is gone: {files:?}");
        assert!(files[0].starts_with(NAME_PREFIX) && files[0].ends_with(".mp4"), "{files:?}");
        let path = fixture.folder().join(&files[0]);
        assert_eq!(saved["data"]["path"], path.display().to_string());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "frames\nfinished\n", "finished before it was moved");

        fixture.recordings.act(saved["data"]["id"].as_u64().unwrap(), ScreenshotAction::Delete).unwrap();
        assert!(fixture.files().is_empty());
        assert!(fixture.recordings.stop().unwrap_err().contains("nothing"));
    }

    #[test]
    fn a_region_with_sound_goes_to_gpu_screen_recorder_as_it_takes_them() {
        let fixture = Fixture::new("gsr-args", GPU_SCREEN_RECORDER, "");
        fixture.start(Some(REGION), RecordAudio { desktop: true, mic: true }).unwrap();
        let args = fixture.args();
        assert!(
            args.starts_with("-w region -region 960x540+2560+-40 -f 60 -fallback-cpu-encoding yes -a default_output|default_input -o "),
            "{args}"
        );
    }

    #[test]
    fn a_gpu_that_cant_encode_records_on_the_cpu() {
        // gpu-screen-recorder 6.1 with an NVENC API older than its FFmpeg
        // wants: without the fallback it finds no encoder and exits 52.
        let fixture = Fixture::new(
            "cpu",
            GPU_SCREEN_RECORDER,
            "case \"$*\" in *'-fallback-cpu-encoding yes'*) ;; *) echo 'no video encoder' >&2; exit 52;; esac",
        );
        fixture.start(None, RecordAudio::default()).unwrap();
        assert_eq!(fixture.next_event()["data"]["output"], "DP-1");
    }

    #[test]
    fn wf_recorder_records_when_gpu_screen_recorder_isnt_there() {
        let fixture = Fixture::new("wf", WF_RECORDER, "");
        fixture.start(Some(REGION), RecordAudio { desktop: true, mic: false }).unwrap();
        let args = fixture.args();
        assert!(
            args.starts_with("-y -g 2560,-40 960x540 -r 60 --audio=@DEFAULT_MONITOR@ --audio-backend=pulse -f "),
            "{args}"
        );
        assert_eq!(fixture.next_event()["data"]["can_pause"], false);
        assert!(fixture.recordings.pause(true).unwrap_err().contains("can't pause"));
    }

    #[test]
    fn pausing_signals_the_recorder_and_leaves_the_pause_out_of_the_time() {
        let fixture = Fixture::new("pause", GPU_SCREEN_RECORDER, "");
        fixture.start(None, RecordAudio::default()).unwrap();
        fixture.next_event();
        fixture.recordings.pause(true).unwrap();
        let paused = fixture.next_event();
        assert_eq!(paused["data"]["paused"], true, "{paused}");
        std::thread::sleep(Duration::from_millis(300));
        fixture.recordings.pause(false).unwrap();
        let resumed = fixture.next_event();
        assert_eq!(resumed["data"]["paused"], false);
        assert!(
            resumed["data"]["elapsed_ms"].as_u64().unwrap() < paused["data"]["elapsed_ms"].as_u64().unwrap() + 200,
            "the pause isn't counted: {paused} then {resumed}"
        );
        nova_test_support::wait_until(WAIT, "the recorder to get SIGUSR2", || {
            std::fs::read_to_string(fixture.dir.join("signals")).unwrap_or_default().lines().count() >= 2
        });
    }

    #[test]
    fn a_recorder_that_fails_at_once_says_why_and_leaves_nothing() {
        let fixture = Fixture::new("fails", GPU_SCREEN_RECORDER, "echo 'no capture permission' >&2; exit 1");
        let error = fixture.start(None, RecordAudio::default()).unwrap_err();
        assert!(error.contains("didn't start") && error.contains("no capture permission"), "{error}");
        assert!(fixture.files().is_empty());
        let queued = crate::hub::queued_names(&fixture.ui);
        assert!(
            !queued.iter().any(|name| name == "recording" || name == "recording_saved"),
            "nothing was announced: {queued:?}"
        );
    }

    #[test]
    fn a_failed_start_says_its_error_line_and_logs_the_rest() {
        let fixture = Fixture::new(
            "why",
            GPU_SCREEN_RECORDER,
            "echo 'gsr info: gsr-kms-server is connected' >&2\n\
             echo 'gsr error: no video encoder was specified and none is supported' >&2\n\
             echo 'gsr info: try -fallback-cpu-encoding yes' >&2; exit 52",
        );
        let error = fixture.start(None, RecordAudio::default()).unwrap_err();
        assert_eq!(error, "gpu-screen-recorder didn't start: no video encoder was specified and none is supported");
        let log = std::fs::read_to_string(nova_paths::log_file(&fixture.dir.join("logs"))).unwrap();
        assert!(log.contains("gsr-kms-server is connected") && log.contains("-fallback-cpu-encoding"), "{log}");
    }

    #[test]
    fn a_failed_start_doesnt_wait_for_what_the_recorder_left_running() {
        let fixture =
            Fixture::new("leaves", GPU_SCREEN_RECORDER, "echo 'no capture permission' >&2; (sleep 5) & exit 1");
        let asked = Instant::now();
        let error = fixture.start(None, RecordAudio::default()).unwrap_err();
        assert!(asked.elapsed() < START_GRACE + child::DRAIN_TIMEOUT * 2, "waited {:?}", asked.elapsed());
        assert!(error.contains("no capture permission"), "{error}");
    }

    #[test]
    fn a_recorder_that_stops_by_itself_still_saves_what_it_recorded() {
        let fixture = Fixture::new("dies", GPU_SCREEN_RECORDER, "(sleep 1.2; kill $$) &");
        fixture.start(None, RecordAudio::default()).unwrap();
        fixture.next_event();
        assert_eq!(fixture.next_event()["data"], serde_json::Value::Null);
        assert_eq!(fixture.next_event()["event"], "recording_saved");
    }

    #[test]
    fn nothing_is_recorded_without_a_recorder_or_into_an_empty_region() {
        let fixture = Fixture::new("none", "something-else", "");
        assert!(fixture.start(None, RecordAudio::default()).unwrap_err().contains("neither"));
        let empty = LayoutRect { width: 0, ..REGION };
        assert!(fixture.start(Some(empty), RecordAudio::default()).unwrap_err().contains("empty"));
    }
}
