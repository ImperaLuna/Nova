use super::*;
use nova_carrier::log::LogLevel;
use nova_hub::{ClientQueue, Outgoing};

struct ScreenshotFiles;

impl SavedFiles for ScreenshotFiles {
    fn time_stamp(&self) -> String {
        nova_screenshot::time_stamp()
    }

    fn place(&self, temporary: &Path, stem: &Path, extension: &str) -> Result<PathBuf, String> {
        nova_screenshot::place(temporary, stem, extension)
    }

    fn act_on(
        &self,
        path: &Path,
        action: ScreenshotAction,
        path_var: &str,
        logger: &Arc<Logger>,
    ) -> Result<(), String> {
        nova_screenshot::act_on(path, action, path_var, logger, LOG_SOURCE)
    }
}
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;

const WAIT: Duration = Duration::from_secs(5);

struct Fixture {
    dir: nova_test_support::TempDir,
    recordings: Recordings,
    ui: Receiver<Outgoing>,
    _ui_stream: UnixStream,
}

impl Fixture {
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
        hub.subscribe(id, nova_recording_api::TOPIC);
        let path_var = nova_test_support::fake_path(&bin, &["sleep", "cp"]);
        let recordings = Recordings::new(path_var, hub, logger, Arc::new(ScreenshotFiles));
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

    fn next_event(&self) -> serde_json::Value {
        nova_hub::next_event(&self.ui, WAIT)
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
    let header = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/recorded/wf-recorder-stalled-on-sound.mp4");
    std::fs::copy(header, folder.join(format!(".{NAME_PREFIX}2026-10-25 18.06.00.{}-1.{EXTENSION}", gone.id())))
        .unwrap();
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
        "an empty one and one without a frame go, and this novad's own and a running one's stay"
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
    let fixture = Fixture::new("thread", GPU_SCREEN_RECORDER, "");
    let (recordings, folder) = (&fixture.recordings, fixture.folder());
    std::thread::scope(|scope| {
        scope.spawn(|| recordings.start("DP-1", None, RecordAudio::default(), &folder).unwrap());
    });
    assert_eq!(fixture.next_event()["data"]["output"], "DP-1");
    nova_test_support::settle(Duration::from_millis(300));
    assert!(lock(&fixture.recordings.state).active.is_some(), "the recorder stopped with its thread");
    let file = fixture.folder().join(&fixture.files()[0]);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "frames\n", "it was told to finish");
}

#[test]
fn a_recorder_that_ignores_the_stop_is_killed_at_the_finish_timeout() {
    let fixture = Fixture::new("stubborn", GPU_SCREEN_RECORDER, "trap '' INT\nwhile :; do sleep 0.02; done");
    fixture.start(None, RecordAudio::default()).unwrap();
    fixture.next_event();
    let stopped = Instant::now();
    fixture.recordings.stop().unwrap();
    assert_eq!(fixture.next_event()["data"], serde_json::Value::Null, "it ended");
    assert!(stopped.elapsed() >= FINISH_TIMEOUT, "killed only at the timeout: {:?}", stopped.elapsed());
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
        args.starts_with(
            "-w region -region 960x540+2560+-40 -f 60 -fallback-cpu-encoding yes -a default_output|default_input -o "
        ),
        "{args}"
    );
}

#[test]
fn a_gpu_that_cant_encode_records_on_the_cpu() {
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
    nova_test_support::settle(Duration::from_millis(300));
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
    let queued = nova_hub::queued_names(&fixture.ui);
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
    let fixture = Fixture::new("leaves", GPU_SCREEN_RECORDER, "echo 'no capture permission' >&2; (sleep 5) & exit 1");
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

fn replaying(name: &str, on_stop: &str) -> String {
    let recorded = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/recorded").join(name);
    format!(
        "for arg; do [ \"$previous\" = -f ] && out=$arg; previous=$arg; done\n\
         cp '{}' \"$out\"\n\
         trap '{on_stop}' INT\n\
         while :; do sleep 0.02; done",
        recorded.display()
    )
}

#[test]
fn a_recording_with_frames_is_saved() {
    let fixture = Fixture::new("frames", WF_RECORDER, &replaying("wf-recorder-saved.mp4", "exit 0"));
    fixture.start(None, RecordAudio::default()).unwrap();
    fixture.next_event();
    fixture.recordings.stop().unwrap();
    assert_eq!(fixture.next_event()["data"], serde_json::Value::Null);
    assert_eq!(fixture.next_event()["event"], "recording_saved");
    assert_eq!(fixture.files().len(), 1);
}

#[test]
fn a_recorder_stalled_on_sound_saves_nothing_and_says_so() {
    let fixture = Fixture::new("stalled", WF_RECORDER, &replaying("wf-recorder-stalled-on-sound.mp4", ""));
    fixture.start(None, RecordAudio { desktop: true, mic: false }).unwrap();
    fixture.next_event();
    fixture.recordings.stop().unwrap();
    assert_eq!(fixture.next_event()["data"], serde_json::Value::Null);
    let failed = fixture.next_event();
    assert_eq!(failed["event"], "recording_failed", "{failed}");
    assert_eq!(failed["data"]["reason"], "wf-recorder stopped before it recorded a frame");
    assert!(fixture.files().is_empty(), "the header is gone: {:?}", fixture.files());
}

#[test]
fn a_finished_recording_without_a_frame_saves_nothing_and_says_so() {
    let fixture = Fixture::new("no-frames", WF_RECORDER, &replaying("wf-recorder-sound-only.mp4", "exit 0"));
    fixture.start(None, RecordAudio { desktop: true, mic: false }).unwrap();
    fixture.next_event();
    fixture.recordings.stop().unwrap();
    assert_eq!(fixture.next_event()["data"], serde_json::Value::Null);
    let failed = fixture.next_event();
    assert_eq!(failed["data"]["reason"], "wf-recorder recorded no frames", "{failed}");
    assert!(fixture.files().is_empty());
}

#[test]
fn nothing_is_recorded_without_a_recorder_or_into_an_empty_region() {
    let fixture = Fixture::new("none", "something-else", "");
    assert!(fixture.start(None, RecordAudio::default()).unwrap_err().contains("neither"));
    let empty = LayoutRect { width: 0, ..REGION };
    assert!(fixture.start(Some(empty), RecordAudio::default()).unwrap_err().contains("empty"));
}
