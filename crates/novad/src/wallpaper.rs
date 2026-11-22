//! Runs the wallpaper feature (`nova-wallpaper`, in `features/wallpaper`)
//! in novad: its module and the answers to its commands. Wiring only: it
//! adapts the feature's core to novad's `Module` trait and `Daemon`, tells
//! it about the compositor's outputs, and all of the feature's work and
//! state stay in the core.

use crate::Daemon;
use nova_base::lock;
use nova_carrier::Outcome;
use nova_socket::{Answer, Caller, done, value};
use nova_wallpaper::WallpaperConfig;
use nova_wallpaper_api::Command;
use std::path::PathBuf;
use std::sync::Arc;

pub use nova_wallpaper::{ENGINE_FEATURE_ID, FEATURE_ID};

/// The feature's core, as novad's module.
pub struct Wallpaper(Arc<nova_wallpaper::Wallpaper>);

impl std::ops::Deref for Wallpaper {
    type Target = nova_wallpaper::Wallpaper;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    let wallpaper = &daemon.kept.wallpaper;
    Answer::Now(match command {
        // The service knows which wallpapers need which feature.
        Command::SetWallpaper { path, output } => done(wallpaper.set(path, output.as_deref())),
        Command::Wallpapers => value(wallpaper.current()),
        Command::WallpaperFolder => return folder(daemon),
    })
}

/// Lists the wallpaper folder on its own thread: reading each file's
/// header takes a moment in a big folder, or one on a slow disk.
fn folder(daemon: &Daemon) -> Answer {
    let folder = match daemon.feature_ready(FEATURE_ID) {
        true => lock(&daemon.settings).wallpaper_config().folder(),
        false => Err(crate::features::not_ready(FEATURE_ID)),
    };
    match folder {
        Ok(folder) => Answer::later(move || value(nova_wallpaper::folder::list(&folder))),
        Err(e) => Answer::Now(Outcome::Error(e)),
    }
}

/// Where the wallpaper service keeps its state, and the home its paths start from.
#[derive(Clone)]
pub struct Sources {
    pub state_dir: PathBuf,
    pub home: Option<PathBuf>,
}

/// Kept for novad's life, told which wallpaper features are ready and what
/// `[wallpaper]` says on every change, and about the outputs whenever they
/// change.
impl crate::modules::Module for Wallpaper {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    type Sources = Sources;
    type Config = WallpaperConfig;

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources { state_dir: env.args.state_dir.clone(), home: env.home.clone() }
    }

    fn config(config: &crate::config::Config) -> WallpaperConfig {
        config.wallpaper.clone()
    }

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        _: &WallpaperConfig,
        _: &[nova_protocol::FeatureState],
    ) -> Arc<Self> {
        let compositor = ctx.compositor.as_ref();
        let wallpaper = nova_wallpaper::Wallpaper::start(
            &sources.state_dir,
            sources.home.clone(),
            ctx.path_var.clone(),
            compositor.is_some(),
            Arc::clone(&ctx.logger),
        );
        if let Some(compositor) = compositor {
            let told = Arc::downgrade(&wallpaper);
            compositor.on_outputs(move |names| {
                if let Some(wallpaper) = told.upgrade() {
                    wallpaper.outputs(names);
                }
            });
        }
        Arc::new(Self(wallpaper))
    }

    fn reconfigure(
        &self,
        _: &WallpaperConfig,
        config: &WallpaperConfig,
        features: &[nova_protocol::FeatureState],
    ) -> crate::modules::Reconfigure {
        let ready = |id| crate::features::is_ready(features, id);
        self.configure(ready(FEATURE_ID), ready(ENGINE_FEATURE_ID), config);
        crate::modules::Reconfigure::InPlace
    }

    fn stop(&self) {
        self.0.stop();
    }
}
