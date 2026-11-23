use crate::Daemon;
use nova_base::lock;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_screenshot_api::{CaptureMode, Command, PixelRect};
use nova_socket::{Answer, Caller, done};
use std::path::PathBuf;
use std::sync::Arc;

use nova_screenshot::FEATURE_ID;

pub struct Screenshots(Arc<nova_screenshot::Screenshots>);

impl std::ops::Deref for Screenshots {
    type Target = nova_screenshot::Screenshots;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match command {
        Command::Screenshot { mode } => screenshot(daemon, *mode),
        Command::SaveCapture { output, rect } => save_capture(daemon, output.clone(), *rect),
        Command::CancelCapture => {
            screenshots(daemon).cancel();
            Answer::Now(done(Ok(())))
        }
        Command::ScreenshotAction { id, action } => Answer::Now(done(screenshots(daemon).act(*id, *action))),
    }
}

fn screenshots(daemon: &Daemon) -> Arc<nova_screenshot::Screenshots> {
    Arc::clone(&daemon.kept.screenshots.0)
}

fn require_feature(daemon: &Daemon) -> Result<(), String> {
    match daemon.feature_ready(FEATURE_ID) {
        true => Ok(()),
        false => Err(nova_modules::not_ready(FEATURE_ID)),
    }
}

fn screenshot(daemon: &Daemon, mode: CaptureMode) -> Answer {
    if let Err(e) = require_feature(daemon) {
        return Answer::Now(Outcome::Error(e));
    }
    let screenshots = screenshots(daemon);
    let compositor = daemon.compositor.clone();
    let folder = lock(&daemon.settings).screenshot_folder();
    Answer::later(move || {
        done(match mode {
            CaptureMode::Region => match compositor.and_then(|compositor| compositor.desktop()) {
                Some(desktop) => {
                    let outputs: Vec<String> = desktop.outputs.into_iter().map(|output| output.name).collect();
                    screenshots.pick_region(&outputs)
                }
                None => Err("novad doesn't know this compositor's screens yet".into()),
            },
            CaptureMode::Screen => {
                let output = compositor.and_then(|compositor| compositor.focused_output());
                folder.and_then(|folder| screenshots.take_screen(output.as_deref(), &folder))
            }
        })
    })
}

fn save_capture(daemon: &Daemon, output: String, rect: PixelRect) -> Answer {
    if let Err(e) = require_feature(daemon) {
        return Answer::Now(Outcome::Error(e));
    }
    let screenshots = screenshots(daemon);
    let folder = lock(&daemon.settings).screenshot_folder();
    Answer::later(move || done(folder.and_then(|folder| screenshots.save_region(&output, rect, &folder))))
}

impl nova_modules::Module for Screenshots {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_screenshot_api::TOPIC);
    type Sources = PathBuf;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> PathBuf {
        env.runtime_dir.to_owned()
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        runtime_dir: &PathBuf,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(Arc::new(nova_screenshot::Screenshots::new(
            runtime_dir,
            ctx.path_var.clone(),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        ))))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {}
}
