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

pub struct Recordings(Arc<nova_recording::Recordings>);

impl std::ops::Deref for Recordings {
    type Target = nova_recording::Recordings;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    let recordings = recordings(daemon);
    match command {
        Command::Record => Answer::Now(done(require_feature(daemon).and_then(|()| recordings.record()))),
        Command::StartRecording { output, region, audio } => start(daemon, output.clone(), *region, *audio),
        Command::StopRecording => Answer::Now(done(recordings.stop())),
        Command::PauseRecording { paused } => Answer::Now(done(recordings.pause(*paused))),
        Command::RecordingAction { id, action } => Answer::Now(done(recordings.act(*id, *action))),
        Command::OpenLog => Answer::Now(done(require_feature(daemon).and_then(|()| recordings.open_log()))),
    }
}

fn recordings(daemon: &Daemon) -> Arc<nova_recording::Recordings> {
    Arc::clone(&daemon.kept.recordings.0)
}

fn require_feature(daemon: &Daemon) -> Result<(), String> {
    match daemon.feature_ready(FEATURE_ID) {
        true => Ok(()),
        false => Err(nova_modules::not_ready(FEATURE_ID)),
    }
}

fn start(daemon: &Daemon, output: String, region: Option<LayoutRect>, audio: RecordAudio) -> Answer {
    if let Err(e) = require_feature(daemon) {
        return Answer::Now(Outcome::Error(e));
    }
    let recordings = recordings(daemon);
    let folder = lock(&daemon.settings).recording_folder();
    Answer::later(move || done(folder.and_then(|folder| recordings.start(&output, region, audio, &folder))))
}

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
