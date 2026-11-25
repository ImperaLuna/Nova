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

pub const FEATURE_ID: &str = "recording";
pub const LOG_SOURCE: &str = "recording";

const GPU_SCREEN_RECORDER: &str = "gpu-screen-recorder";
const WF_RECORDER: &str = "wf-recorder";
const START_GRACE: Duration = Duration::from_millis(800);
const FINISH_TIMEOUT: Duration = if cfg!(test) { Duration::from_secs(1) } else { Duration::from_secs(10) };
const ERROR_MARK: &str = "error: ";
const FPS: &str = "60";
const GSR_CPU_FALLBACK: [&str; 2] = ["-fallback-cpu-encoding", "yes"];
const KEEP_SAVED: usize = 32;
const NAME_PREFIX: &str = "Recording ";
const EXTENSION: &str = "mp4";
const GSR_DESKTOP: &str = "default_output";
const GSR_MIC: &str = "default_input";
const WF_DESKTOP: &str = "@DEFAULT_MONITOR@";
// wf-recorder's PipeWire backend records the desktop by default; the default monitor's name is the pulse backend's:
// https://github.com/ammen99/wf-recorder
const WF_AUDIO_BACKEND: &str = "pulse";

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RecordingConfig {
    #[serde(default)]
    pub folder: Option<String>,
}

impl RecordingConfig {
    pub fn folder(&self) -> Result<PathBuf, String> {
        nova_settings::folder("recording", self.folder.as_deref(), || {
            nova_paths::user_dir("VIDEOS", "Videos").join(DEFAULT_FOLDER)
        })
    }
}

const DEFAULT_FOLDER: &str = "Recordings";

pub trait SavedFiles: Send + Sync + 'static {
    fn time_stamp(&self) -> String;
    fn place(&self, temporary: &Path, stem: &Path, extension: &str) -> Result<PathBuf, String>;
    fn act_on(&self, path: &Path, action: ScreenshotAction, path_var: &str, logger: &Arc<Logger>)
    -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recorder {
    Gpu,
    Wf,
}

pub struct Recordings {
    path_var: String,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    files: Arc<dyn SavedFiles>,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    allowed: bool,
    active: Option<Active>,
    saved: VecDeque<SavedRecording>,
    next_id: u64,
    runs: u64,
}

struct Active {
    run: u64,
    pid: libc::pid_t,
    recorder: Recorder,
    output: String,
    region: Option<LayoutRect>,
    audio: RecordAudio,
    file: PathBuf,
    stem: PathBuf,
    started: Instant,
    paused_since: Option<Instant>,
    paused_for: Duration,
    stopping: bool,
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

    fn signal(&self, signal: libc::c_int) {
        // SAFETY: kill takes no pointers.
        unsafe { libc::kill(self.pid, signal) };
    }
}

impl Recordings {
    pub fn new(path_var: String, hub: Arc<Hub>, logger: Arc<Logger>, files: Arc<dyn SavedFiles>) -> Self {
        Self { path_var, hub, logger, files, state: Arc::default() }
    }

    pub fn record(&self) -> Result<(), String> {
        if lock(&self.state).active.is_some() {
            return self.stop();
        }
        match self.hub.publish(&Event::PickRecording) {
            0 => Err("the UI isn't connected to novad, so there's nothing to pick what to record with".into()),
            _ => Ok(()),
        }
    }

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
        child::die_with_novad(&mut command, libc::SIGINT);
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

    pub fn stop(&self) -> Result<(), String> {
        self.stop_in(&mut lock(&self.state))
    }

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

    pub fn pause(&self, paused: bool) -> Result<(), String> {
        let mut state = lock(&self.state);
        let active = state.active.as_mut().ok_or("nothing is being recorded")?;
        if active.recorder != Recorder::Gpu {
            return Err(format!("{} can't pause", active.recorder.name()));
        }
        if active.paused_since.is_some() == paused || active.stopping {
            return Ok(());
        }
        active.signal(libc::SIGUSR2);
        match active.paused_since.take() {
            Some(since) => active.paused_for += since.elapsed(),
            None => active.paused_since = Some(Instant::now()),
        }
        self.hub.publish(&Event::Recording(Some(active.event())));
        Ok(())
    }

    pub fn send_state(&self, client: ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(client, &Event::Recording(state.active.as_ref().map(Active::event)));
    }

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

    pub fn open_log(&self) -> Result<(), String> {
        nova_apps::open(&nova_paths::log_file(&self.logger.dir()), &self.path_var, &self.logger)
    }

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

// The recorder's death signal comes when the thread that spawned it ends, so it is spawned from this thread:
// https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html
struct Keeper {
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
            let _ = child.kill();
        }
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
        let written = std::fs::metadata(&active.file).is_ok_and(|meta| meta.len() > 0);
        if !written || mp4::contents(&active.file) == mp4::Contents::NoFrames {
            let _ = std::fs::remove_file(&active.file);
            let reason = if finished {
                format!("{} recorded no frames", active.recorder.name())
            } else {
                format!("{} stopped before it recorded a frame", active.recorder.name())
            };
            self.logger.warn(LOG_SOURCE, &format!("nothing was recorded: {reason}"));
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

fn running(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
}

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
