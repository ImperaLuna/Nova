//! Runs the recording feature (`nova-recording`, in `features/recording`)
//! in novad: its kept module and the answers to its commands. Wiring only:
//! it adapts the feature's core to novad's `Module` trait and `Daemon`, and
//! fills its [`SavedFiles`](nova_recording::SavedFiles) port with the
//! screenshot feature's helpers. All of the feature's work and state stay
//! in the core.

use crate::Daemon;
use nova_base::lock;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_recording::{RecordingConfig, SavedFiles};
use nova_recording_api::{Command, LayoutRect, RecordAudio, ScreenshotAction};
use nova_socket::{Answer, Caller, done};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_recording::FEATURE_ID;

/// The feature's core, as novad's module.
pub struct Recordings(Arc<nova_recording::Recordings>);

impl std::ops::Deref for Recordings {
    type Target = nova_recording::Recordings;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    let recordings = recordings(daemon);
    match command {
        Command::Record => Answer::Now(done(require_feature(daemon).and_then(|()| recordings.record()))),
        Command::StartRecording { output, region, audio } => start(daemon, output.clone(), *region, *audio),
        // Even with the feature off, so a reload can't leave one running.
        Command::StopRecording => Answer::Now(done(recordings.stop())),
        Command::PauseRecording { paused } => Answer::Now(done(recordings.pause(*paused))),
        Command::RecordingAction { id, action } => Answer::Now(done(recordings.act(*id, *action))),
        Command::OpenLog => Answer::Now(done(require_feature(daemon).and_then(|()| recordings.open_log()))),
    }
}

/// Kept for novad's life, whatever the settings.
fn recordings(daemon: &Daemon) -> Arc<nova_recording::Recordings> {
    Arc::clone(&daemon.kept.recordings.0)
}

/// An error saying the feature isn't ready, when it isn't.
fn require_feature(daemon: &Daemon) -> Result<(), String> {
    match daemon.feature_ready(FEATURE_ID) {
        true => Ok(()),
        false => Err(nova_modules::not_ready(FEATURE_ID)),
    }
}

/// Starts the recording the UI's picker asked for.
fn start(daemon: &Daemon, output: String, region: Option<LayoutRect>, audio: RecordAudio) -> Answer {
    if let Err(e) = require_feature(daemon) {
        return Answer::Now(Outcome::Error(e));
    }
    let recordings = recordings(daemon);
    let folder = lock(&daemon.settings).recording_folder();
    Answer::later(move || done(folder.and_then(|folder| recordings.start(&output, region, audio, &folder))))
}

/// The screenshot feature's helpers, as the recording feature's
/// [`SavedFiles`] port.
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
        logger: &Arc<nova_log::Logger>,
    ) -> Result<(), String> {
        nova_screenshot::act_on(path, action, path_var, logger, nova_recording::LOG_SOURCE)
    }
}

/// Kept for novad's life, so a reload can't leave a recording running:
/// turning the feature off stops it.
impl nova_modules::Module for Recordings {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_recording_api::TOPIC);
    type Sources = ();
    type Config = RecordingConfig;

    fn sources(_: &crate::modules::Env) {}

    fn config(config: &crate::config::Config) -> RecordingConfig {
        config.recording.clone()
    }

    /// Saves what an earlier novad left in the folder, when the feature is
    /// ready. Allowed to record once told the settings (`reconfigure`).
    fn start(
        ctx: &crate::modules::Context,
        (): &(),
        config: &RecordingConfig,
        features: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        let recordings = Arc::new(nova_recording::Recordings::new(
            ctx.path_var.clone(),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
            Arc::new(ScreenshotFiles),
        ));
        if crate::features::is_ready(features, FEATURE_ID)
            && let Ok(folder) = config.folder()
        {
            recordings.recover(&folder);
        }
        Arc::new(Self(recordings))
    }

    fn reconfigure(
        &self,
        _: &RecordingConfig,
        _: &RecordingConfig,
        features: &[nova_daemon_info::FeatureState],
    ) -> nova_modules::Reconfigure {
        self.allow(crate::features::is_ready(features, FEATURE_ID));
        nova_modules::Reconfigure::InPlace
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {}
}
