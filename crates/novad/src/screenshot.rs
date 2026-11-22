//! Runs the screenshot feature (`nova-screenshot`, in
//! `features/screenshot`) in novad: its kept module and the answers to its
//! commands. Wiring only: it adapts the feature's core to novad's `Module`
//! trait and `Daemon`, and all of the feature's work and state stay in the
//! core.

use crate::hub::ClientId;
use crate::{Daemon, lock};
use nova_protocol::Outcome;
use nova_screenshot_api::{CaptureMode, Command, PixelRect};
use nova_socket::{Answer, Caller, done};
use std::path::PathBuf;
use std::sync::Arc;

pub use nova_screenshot::FEATURE_ID;

/// The feature's core, as novad's module.
pub struct Screenshots(Arc<nova_screenshot::Screenshots>);

impl std::ops::Deref for Screenshots {
    type Target = nova_screenshot::Screenshots;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    match command {
        Command::Screenshot { mode } => screenshot(daemon, *mode),
        Command::SaveCapture { output, rect } => save_capture(daemon, output.clone(), *rect),
        // Even with the feature off, so a reload can't strand the picker.
        Command::CancelCapture => {
            screenshots(daemon).cancel();
            Answer::Now(done(Ok(())))
        }
        Command::ScreenshotAction { id, action } => Answer::Now(done(screenshots(daemon).act(*id, *action))),
    }
}

/// Kept for novad's life, whatever the settings.
fn screenshots(daemon: &Daemon) -> Arc<nova_screenshot::Screenshots> {
    Arc::clone(&daemon.kept.screenshots.0)
}

/// An error saying the feature isn't ready, when it isn't.
fn require_feature(daemon: &Daemon) -> Result<(), String> {
    match daemon.feature_ready(FEATURE_ID) {
        true => Ok(()),
        false => Err(crate::features::not_ready(FEATURE_ID)),
    }
}

/// Opens the region picker on every screen (or closes it), or saves the
/// focused screen.
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

/// Saves the region the user picked.
fn save_capture(daemon: &Daemon, output: String, rect: PixelRect) -> Answer {
    if let Err(e) = require_feature(daemon) {
        return Answer::Now(Outcome::Error(e));
    }
    let screenshots = screenshots(daemon);
    let folder = lock(&daemon.settings).screenshot_folder();
    Answer::later(move || done(folder.and_then(|folder| screenshots.save_region(&output, rect, &folder))))
}

/// Kept for novad's life, so a reload can't strand the region picker.
impl crate::modules::Module for Screenshots {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_screenshot_api::TOPIC);
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
        _: &[nova_protocol::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(Arc::new(nova_screenshot::Screenshots::new(
            runtime_dir,
            ctx.path_var.clone(),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        ))))
    }

    /// A restarted UI picks up a region picker that was open.
    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {}
}
