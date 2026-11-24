use crate::config::{self, Config};
use crate::features;
use crate::modules::{self, Context, Running};
use nova_compositor::Compositor;
use nova_daemon_info::{Event, FeatureState};
use nova_features::{Manifest, Selection};
use nova_host_info::Host;
use nova_hub::Hub;
use nova_icons::Icons;
use nova_log::Logger;
use nova_theme_info::Theme;
use std::path::PathBuf;
use std::sync::Arc;

const LOG_SOURCE: &str = "settings";

pub struct Sources {
    pub config: PathBuf,
    pub selection: Vec<PathBuf>,
    pub themes_dir: PathBuf,
    pub manifests: Vec<Manifest>,
    pub path_var: String,
    pub modules: modules::Sources,
    pub icons: Arc<Icons>,
}

pub struct Settings {
    config: Config,
    selection: Selection,
    pub features: Vec<FeatureState>,
    pub theme: Theme,
    pub context: Context,
    pub modules: Running,
}

impl Settings {
    pub fn load(sources: &Sources, hub: &Arc<Hub>, logger: &Arc<Logger>, compositor: Option<Arc<Compositor>>) -> Self {
        let fallback = "broken settings, using the defaults";
        let config = logged(config::read(&sources.config), fallback, logger).unwrap_or_default();
        let selection = logged(features::read_selection(&sources.selection), fallback, logger).unwrap_or_default();
        warn_unknown_modes(&sources.manifests, &selection, logger);
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

    pub fn stop_modules(&mut self) {
        self.modules.stop_all();
        self.context.apps.forget();
    }

    pub fn host(&self) -> Host {
        nova_host::read(self.config.quick_settings.name.as_deref(), &self.context.icons)
    }

    pub fn screenshot_folder(&self) -> Result<PathBuf, String> {
        self.config.screenshot.folder()
    }

    pub fn recording_folder(&self) -> Result<PathBuf, String> {
        self.config.recording.folder()
    }

    pub fn wallpaper_config(&self) -> &nova_wallpaper::WallpaperConfig {
        &self.config.wallpaper
    }
}

fn resolve_theme(sources: &Sources, config: &Config, logger: &Logger) -> Theme {
    let (theme, problems) = nova_theme::resolve(&sources.themes_dir, &config.theme);
    for problem in problems {
        logger.warn("theme", &problem);
    }
    logger.info("theme", &format!("using {} ({:?})", theme.id, theme.mode));
    theme
}

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

fn warn_unknown_modes(manifests: &[Manifest], selection: &Selection, logger: &Logger) {
    for (id, mode) in selection.unknown_modes(manifests) {
        logger.warn("features", &format!("{id} has no mode \"{mode}\" (features.toml), so it runs in its default"));
    }
}

fn logged<T>(read: Result<T, String>, fallback: &str, logger: &Logger) -> Option<T> {
    read.map_err(|problem| logger.error(LOG_SOURCE, &format!("{fallback}: {problem}"))).ok()
}
