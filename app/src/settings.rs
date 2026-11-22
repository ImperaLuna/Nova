//! The user's settings, `config.toml` and `features.toml`: read at start and
//! again whenever either file changes, so edits apply without a restart.
//! Both hand the features' states and the config to the module registry
//! (`crate::modules`), whose one loop starts, stops and reconfigures
//! modules, so a reload ends where a fresh start with the same files would.
//!
//! A file that doesn't parse means defaults at start, and keeps the last good
//! version after that, so a half-finished edit never blanks the UI. Either
//! way the problem is logged.

use crate::config::{self, Config};
use crate::features;
use crate::modules::{self, Context, Running};
use nova_cli::client::RESTART_COMMAND;
use nova_compositor::Compositor;
use nova_daemon_info::{Event, FeatureState, UiProgram};
use nova_features::{Manifest, Selection};
use nova_host_info::Host;
use nova_hub::Hub;
use nova_icons::Icons;
use nova_log::Logger;
use nova_theme_info::Theme;
use std::path::PathBuf;
use std::sync::Arc;

const LOG_SOURCE: &str = "settings";

/// Where the settings come from, fixed for novad's life.
pub struct Sources {
    /// `config.toml` (see `config`).
    pub config: PathBuf,
    /// `features.toml`: the first of these that exists.
    pub selection: Vec<PathBuf>,
    pub themes_dir: PathBuf,
    pub manifests: Vec<Manifest>,
    /// Where features' binaries are looked for.
    pub path_var: String,
    /// Where each module reads and keeps things.
    pub modules: modules::Sources,
    /// Where app icons are found, for every feature that shows them.
    pub icons: Arc<Icons>,
}

/// The settings in effect, and the modules they run.
pub struct Settings {
    config: Config,
    selection: Selection,
    pub features: Vec<FeatureState>,
    pub theme: Theme,
    /// What the modules share.
    pub context: Context,
    /// The modules running for these settings.
    pub modules: Running,
}

impl Settings {
    /// The settings at start: a missing or broken file means defaults. The
    /// modules start before the features are published, so the UI that
    /// loads a feature on that event finds its module running.
    pub fn load(sources: &Sources, hub: &Arc<Hub>, logger: &Arc<Logger>, compositor: Option<Arc<Compositor>>) -> Self {
        let fallback = "broken settings, using the defaults";
        let config = logged(config::read(&sources.config), fallback, logger).unwrap_or_default();
        let selection = logged(features::read_selection(&sources.selection), fallback, logger).unwrap_or_default();
        warn_unknown_modes(&sources.manifests, &selection, logger);
        warn_unknown_ui(&selection, logger);
        let features = features::states(&sources.manifests, &selection, &sources.path_var);
        let theme = resolve_theme(sources, &config, logger);
        hub.publish(&nova_theme_info::Event::Theme(theme.clone()));
        let context = Context {
            hub: Arc::clone(hub),
            logger: Arc::clone(logger),
            path_var: sources.path_var.clone(),
            icons: Arc::clone(&sources.icons),
            compositor,
            apps: nova_apps::AppIndex::new(sources.modules.apps.clone(), &sources.path_var, Arc::clone(logger)),
        };
        let modules = Running::start(&context, &sources.modules, &config, &features);
        publish_features(&[], &features, hub, logger);
        Self { config, selection, features, theme, context, modules }
    }

    /// Reads both files again and applies what changed. A broken file keeps
    /// its last good version. Anything that differs is logged and
    /// published, and modules are started, stopped or restarted to match.
    pub fn reload(&mut self, sources: &Sources, hub: &Arc<Hub>, logger: &Arc<Logger>) {
        let fallback = "broken settings, keeping the last good ones";
        let config = logged(config::read(&sources.config), fallback, logger).unwrap_or_else(|| self.config.clone());
        let selection = logged(features::read_selection(&sources.selection), fallback, logger)
            .unwrap_or_else(|| self.selection.clone());
        if config == self.config && selection == self.selection {
            return;
        }
        logger.info(LOG_SOURCE, "settings changed, applying them");
        warn_unknown_modes(&sources.manifests, &selection, logger);
        if selection.ui != self.selection.ui {
            warn_unknown_ui(&selection, logger);
            // The supervisor keeps the program it started with.
            logger.info(LOG_SOURCE, &format!("the UI program changed (features.toml): `{RESTART_COMMAND}` applies it"));
        }
        let features = features::states(&sources.manifests, &selection, &sources.path_var);
        if self.config.theme != config.theme {
            let theme = resolve_theme(sources, &config, logger);
            if theme != self.theme {
                hub.publish(&nova_theme_info::Event::Theme(theme.clone()));
            }
            self.theme = theme;
        }
        if self.config.quick_settings != config.quick_settings {
            hub.publish(&nova_host_info::Event::Host(nova_host::read(
                config.quick_settings.name.as_deref(),
                &sources.icons,
            )));
        }
        self.modules.apply(&self.context, &sources.modules, &config, &features);
        publish_features(&self.features, &features, hub, logger);
        self.config = config;
        self.selection = selection;
        self.features = features;
    }

    /// The UI program `features.toml` picks; Quickshell unless it names
    /// another one novad knows.
    pub fn ui_program(&self) -> UiProgram {
        self.selection.ui_program()
    }

    /// Stops every module, for a novad that ends without exiting the
    /// process ([`crate::InProcess`]).
    pub fn stop_modules(&mut self) {
        self.modules.stop_all();
        self.context.apps.forget();
    }

    /// The quick settings header's facts, with the config's name for the
    /// user if it has one.
    pub fn host(&self) -> Host {
        nova_host::read(self.config.quick_settings.name.as_deref(), &self.context.icons)
    }

    /// Where screenshots are saved, from `config.toml`.
    pub fn screenshot_folder(&self) -> Result<PathBuf, String> {
        self.config.screenshot.folder()
    }

    /// Where recordings are saved, from `config.toml`.
    pub fn recording_folder(&self) -> Result<PathBuf, String> {
        self.config.recording.folder()
    }

    /// The wallpapers set in `config.toml`.
    pub fn wallpaper_config(&self) -> &nova_wallpaper::WallpaperConfig {
        &self.config.wallpaper
    }
}

/// The theme `config` picks, its problems logged.
fn resolve_theme(sources: &Sources, config: &Config, logger: &Logger) -> Theme {
    let (theme, problems) = nova_theme::resolve(&sources.themes_dir, &config.theme);
    for problem in problems {
        logger.warn("theme", &problem);
    }
    logger.info("theme", &format!("using {} ({:?})", theme.id, theme.mode));
    theme
}

/// Logs the features whose state changed from `before`, and publishes them
/// all when any did.
fn publish_features(before: &[FeatureState], features: &[FeatureState], hub: &Hub, logger: &Logger) {
    for feature in features.iter().filter(|feature| !before.contains(feature)) {
        match &feature.mode {
            Some(mode) => logger.info("features", &format!("{} is {:?} ({mode})", feature.id, feature.status)),
            None => logger.info("features", &format!("{} is {:?}", feature.id, feature.status)),
        }
    }
    if before != features {
        hub.publish(&Event::Features { features: features.to_vec() });
    }
}

/// Warns of `[modes]` picks in `features.toml` that name no mode of their
/// feature, which then runs in its default mode.
fn warn_unknown_modes(manifests: &[Manifest], selection: &Selection, logger: &Logger) {
    for (id, mode) in selection.unknown_modes(manifests) {
        logger.warn("features", &format!("{id} has no mode \"{mode}\" (features.toml), so it runs in its default"));
    }
}

/// Warns of a `ui` in `features.toml` that names no UI program, so
/// Quickshell runs.
fn warn_unknown_ui(selection: &Selection, logger: &Logger) {
    if let Some(name) = selection.unknown_ui() {
        let known = UiProgram::ALL.map(UiProgram::name).join(", ");
        logger.warn(
            "features",
            &format!("there is no \"{name}\" UI (features.toml; known: {known}), so Quickshell runs"),
        );
    }
}

/// `read`'s settings, or `None` after logging why not and what happens
/// instead. The outcome goes first: a TOML error spans several lines.
fn logged<T>(read: Result<T, String>, fallback: &str, logger: &Logger) -> Option<T> {
    read.map_err(|problem| logger.error(LOG_SOURCE, &format!("{fallback}: {problem}"))).ok()
}
