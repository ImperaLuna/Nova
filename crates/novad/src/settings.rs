//! The user's settings, `config.toml` and `features.toml`: read at start and
//! again whenever either file changes, so edits apply without a restart.
//! Starting and reloading both go through `Settings::apply`, so a reload ends
//! where a fresh start with the same files would.
//!
//! A file that doesn't parse means defaults at start, and keeps the last good
//! version after that, so a half-finished edit never blanks the UI. Either
//! way the problem is logged.

use crate::config::{self, Config};
use crate::hub::Hub;
use crate::icons::Icons;
use crate::inotify::{self, Inotify};
use crate::logger::Logger;
use crate::{ai_usage, features, host, launcher, notifications, resources, theme, tray, x11_tray};
use nova_features::{Manifest, Selection};
use nova_protocol::{Event, FeatureState, Host, Theme};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

const LOG_SOURCE: &str = "settings";
/// Editors save in steps (write a copy, move the old file away, rename the
/// copy into place); a reload waits until changes pause this long.
const SETTLE: Duration = Duration::from_millis(200);

/// Where the settings come from, fixed for novad's life.
pub struct Sources {
    /// `config.toml` (see `config`).
    pub config: PathBuf,
    /// `features.toml`.
    pub selection: PathBuf,
    pub themes_dir: PathBuf,
    pub manifests: Vec<Manifest>,
    /// Where features' binaries are looked for.
    pub path_var: String,
    pub ai_usage: ai_usage::Paths,
    pub resources: resources::Sources,
    pub launcher: launcher::Paths,
    pub tray: tray::Sources,
    pub notifications: notifications::Sources,
    /// Where app icons are found, for every feature that shows them.
    pub icons: Arc<Icons>,
}

/// The settings in effect, and the feature services they started.
pub struct Settings {
    config: Config,
    selection: Selection,
    pub features: Vec<FeatureState>,
    pub theme: Theme,
    /// Running while the `ai-usage` feature is ready.
    pub ai_usage: Option<Arc<ai_usage::Usage>>,
    /// Sampling while the `quick-settings` feature is ready.
    pub resources: Option<Arc<resources::Resources>>,
    /// Running while the `launcher` feature is ready.
    pub launcher: Option<Arc<launcher::Launcher>>,
    /// Watching for tray icons while the `tray` feature is ready.
    pub tray: Option<Arc<tray::Tray>>,
    /// Running xembedsniproxy while the `x11-tray` feature is ready.
    pub x11_tray: Option<Arc<x11_tray::X11Tray>>,
    /// Serving `org.freedesktop.Notifications` while the `notifications` feature is ready.
    pub notifications: Option<Arc<notifications::Notifications>>,
    /// For the quick settings header's logo.
    icons: Arc<Icons>,
}

impl Settings {
    /// The settings at start: a missing or broken file means defaults.
    pub fn load(sources: &Sources, hub: &Arc<Hub>, logger: &Arc<Logger>) -> Self {
        let fallback = "broken settings, using the defaults";
        let config = logged(config::read(&sources.config), fallback, logger).unwrap_or_default();
        let selection = logged(features::read_selection(&sources.selection), fallback, logger).unwrap_or_default();
        Self::apply(config, selection, None, sources, hub, logger)
    }

    /// Reads both files again and applies what changed. A broken file keeps
    /// its last good version.
    pub fn reload(&mut self, sources: &Sources, hub: &Arc<Hub>, logger: &Arc<Logger>) {
        let fallback = "broken settings, keeping the last good ones";
        let config = logged(config::read(&sources.config), fallback, logger).unwrap_or_else(|| self.config.clone());
        let selection = logged(features::read_selection(&sources.selection), fallback, logger)
            .unwrap_or_else(|| self.selection.clone());
        if config == self.config && selection == self.selection {
            return;
        }
        logger.info(LOG_SOURCE, "settings changed, applying them");
        let next = Self::apply(config, selection, Some(self), sources, hub, logger);
        *self = next;
    }

    /// Settings for `config` and `selection`. Anything that differs from
    /// `previous` is logged and published, and feature services are started,
    /// stopped or restarted to match.
    fn apply(
        config: Config,
        selection: Selection,
        mut previous: Option<&mut Self>,
        sources: &Sources,
        hub: &Arc<Hub>,
        logger: &Arc<Logger>,
    ) -> Self {
        let features = features::states(&sources.manifests, &selection, &sources.path_var);
        let theme = match previous.as_deref() {
            Some(previous) if previous.config.theme == config.theme => previous.theme.clone(),
            _ => {
                let (theme, problems) = theme::resolve(&sources.themes_dir, &config.theme);
                for problem in problems {
                    logger.warn("theme", &problem);
                }
                logger.info("theme", &format!("using {} ({:?})", theme.id, theme.mode));
                theme
            }
        };
        if previous.as_deref().is_none_or(|previous| previous.theme != theme) {
            hub.publish(&Event::Theme(theme.clone()));
        }
        if previous.as_deref().is_some_and(|previous| previous.config.quick_settings != config.quick_settings) {
            hub.publish(&Event::Host(host::read(config.quick_settings.name.as_deref(), &sources.icons)));
        }

        let ready = |id| features::is_ready(&features, id);
        // Started before the features are published, so the UI that loads
        // the feature on that event gets its numbers. Restarted when its
        // options change.
        let ai_usage_settings_kept =
            previous.as_deref().is_some_and(|previous| previous.config.ai_usage == config.ai_usage);
        let ai_usage = reconcile(
            ai_usage::FEATURE_ID,
            previous.as_deref_mut().and_then(|previous| previous.ai_usage.take()),
            ready(ai_usage::FEATURE_ID),
            |_| ai_usage_settings_kept,
            || ai_usage::Usage::start(sources.ai_usage.clone(), &config.ai_usage, Arc::clone(hub), Arc::clone(logger)),
            logger,
        );
        let resources = reconcile(
            resources::FEATURE_ID,
            previous.as_deref_mut().and_then(|previous| previous.resources.take()),
            ready(resources::FEATURE_ID),
            |_| true,
            || resources::Resources::start(sources.resources.clone(), Arc::clone(hub), Arc::clone(logger)),
            logger,
        );
        // It has no options: it only starts and stops with its feature.
        let launcher = reconcile(
            launcher::FEATURE_ID,
            previous.as_deref_mut().and_then(|previous| previous.launcher.take()),
            ready(launcher::FEATURE_ID),
            |_| true,
            || {
                launcher::Launcher::start(
                    sources.launcher.clone(),
                    &sources.path_var,
                    Arc::clone(hub),
                    Arc::clone(logger),
                )
            },
            logger,
        );
        // Started before the UI, which settings start ahead of, so apps it
        // autostarts find the watcher. It finds apps with the launcher, which
        // a running tray is handed again (it may have just started or
        // stopped).
        let tray = reconcile(
            tray::FEATURE_ID,
            previous.as_deref_mut().and_then(|previous| previous.tray.take()),
            ready(tray::FEATURE_ID),
            |running| {
                running.set_launcher(launcher.clone());
                true
            },
            || {
                tray::Tray::start(
                    sources.tray.clone(),
                    sources.resources.proc_root.clone(),
                    launcher.clone(),
                    Arc::clone(hub),
                    Arc::clone(logger),
                )
            },
            logger,
        );
        // Started before the UI, which settings start ahead of, so
        // notifications sent while it starts are kept. No options either.
        let notifications = reconcile(
            notifications::FEATURE_ID,
            previous.as_deref_mut().and_then(|previous| previous.notifications.take()),
            ready(notifications::FEATURE_ID),
            |_| true,
            || notifications::Notifications::start(sources.notifications.clone(), Arc::clone(hub), Arc::clone(logger)),
            logger,
        );
        if let Some(launcher) = &launcher {
            let tray = tray.as_ref().map(Arc::downgrade);
            launcher.on_rescan(move || {
                if let Some(tray) = tray.as_ref().and_then(std::sync::Weak::upgrade) {
                    tray.apps_changed();
                }
            });
        }
        // It has no options either. Its icons reach whatever tray is up.
        let x11_tray = reconcile(
            x11_tray::FEATURE_ID,
            previous.as_deref_mut().and_then(|previous| previous.x11_tray.take()),
            ready(x11_tray::FEATURE_ID),
            |_| true,
            || x11_tray::X11Tray::start(sources.tray.bus.clone(), &sources.path_var, Arc::clone(logger)),
            logger,
        );

        let before = previous.as_deref().map(|previous| previous.features.as_slice()).unwrap_or_default();
        for feature in features.iter().filter(|feature| !before.contains(feature)) {
            logger.info("features", &format!("{} is {:?}", feature.id, feature.status));
        }
        if before != features {
            hub.publish(&Event::Features { features: features.clone() });
        }

        let icons = Arc::clone(&sources.icons);
        Self { config, selection, features, theme, ai_usage, resources, launcher, tray, x11_tray, notifications, icons }
    }
}

impl Settings {
    /// Stops every feature service running, for a novad that ends without
    /// exiting the process ([`crate::InProcess`]).
    pub fn stop_services(&self) {
        let services: [Option<&dyn Service>; 6] = [
            self.ai_usage.as_deref().map(|service| service as &dyn Service),
            self.resources.as_deref().map(|service| service as &dyn Service),
            self.launcher.as_deref().map(|service| service as &dyn Service),
            self.tray.as_deref().map(|service| service as &dyn Service),
            self.x11_tray.as_deref().map(|service| service as &dyn Service),
            self.notifications.as_deref().map(|service| service as &dyn Service),
        ];
        for service in services.into_iter().flatten() {
            service.stop();
        }
    }

    /// The quick settings header's facts, with the config's name for the
    /// user if it has one.
    pub fn host(&self) -> Host {
        host::read(self.config.quick_settings.name.as_deref(), &self.icons)
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
    pub fn wallpaper_config(&self) -> &config::WallpaperConfig {
        &self.config.wallpaper
    }
}

/// A feature service that settings start and stop.
pub trait Service {
    fn stop(&self);
}

macro_rules! services {
    ($($service:ty),*) => {
        $(impl Service for $service {
            fn stop(&self) {
                <$service>::stop(self);
            }
        })*
    };
}
services!(
    ai_usage::Usage,
    resources::Resources,
    launcher::Launcher,
    tray::Tray,
    notifications::Notifications,
    x11_tray::X11Tray
);

/// The service for feature `id` after a change: `running` (from the
/// previous settings) is kept while the feature is `ready` and `keep` says
/// so, else stopped; a new one is `start`ed while the feature is ready and
/// none is kept.
fn reconcile<S: Service>(
    id: &str,
    running: Option<Arc<S>>,
    ready: bool,
    keep: impl FnOnce(&Arc<S>) -> bool,
    start: impl FnOnce() -> Arc<S>,
    logger: &Logger,
) -> Option<Arc<S>> {
    match running {
        Some(running) if ready && keep(&running) => return Some(running),
        Some(running) => {
            running.stop();
            logger.info("features", &format!("stopped {id}"));
        }
        None => {}
    }
    ready.then(|| {
        logger.info("features", &format!("starting {id}"));
        start()
    })
}

/// `read`'s settings, or `None` after logging why not and what happens
/// instead. The outcome goes first: a TOML error spans several lines.
fn logged<T>(read: Result<T, String>, fallback: &str, logger: &Logger) -> Option<T> {
    read.map_err(|problem| logger.error(LOG_SOURCE, &format!("{fallback}: {problem}"))).ok()
}

/// Calls `reload` on a thread of its own once `files` stop changing. The
/// folders around them are watched rather than the files, so a file that's
/// replaced (saved by rename, a new symlink) is still seen, and so is a
/// folder that appears later or is replaced. Watching ends when the
/// returned stopper is used.
pub fn watch(files: Vec<PathBuf>, logger: Arc<Logger>, reload: impl Fn() + Send + 'static) -> Option<inotify::Stopper> {
    let mut watcher = match Inotify::new(inotify::REPLACEMENTS) {
        Ok(watcher) => watcher,
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("can't watch the settings ({e}); changes apply after `nova restart`"));
            return None;
        }
    };
    let stopper = watcher.stopper();
    let mut entries = watch_entries(&mut watcher, &files, &logger);

    let (changed, changes) = mpsc::channel();
    // The settings were read before the watches were in place: read them
    // once more, so an edit in between isn't missed. Unchanged files apply
    // nothing.
    let _ = changed.send(());
    let watch_logger = Arc::clone(&logger);
    std::thread::spawn(move || {
        loop {
            let found = match watcher.read() {
                Ok(Some(found)) => found,
                Ok(None) => return,
                Err(e) => {
                    watch_logger.warn(LOG_SOURCE, &format!("stopped watching the settings: {e}"));
                    return;
                }
            };
            if !found.overflowed && !found.found.iter().any(|change| entries.contains(&change.path)) {
                continue;
            }
            // A folder or link may have changed, so look again at what to watch.
            entries = watch_entries(&mut watcher, &files, &watch_logger);
            if changed.send(()).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        while changes.recv().is_ok() {
            // Settled, unless the watcher stopped meanwhile: then nothing
            // more applies (an in-process novad stops its services next).
            loop {
                match changes.recv_timeout(SETTLE) {
                    Ok(()) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            reload();
        }
    });
    Some(stopper)
}

/// Watches every folder holding an entry that decides what `files` read,
/// and returns those entries: each file and the file it links to (a dotfiles
/// checkout, say), and their folders, which may appear later, be replaced or
/// be links themselves. Watching a folder that's already watched is harmless.
fn watch_entries(watcher: &mut Inotify, files: &[PathBuf], logger: &Logger) -> BTreeSet<PathBuf> {
    let mut entries = BTreeSet::new();
    for file in files {
        for entry in [Some(file.clone()), std::fs::canonicalize(file).ok()].into_iter().flatten() {
            entries.extend(entry.parent().map(Path::to_owned));
            entries.insert(entry);
        }
    }
    let folders: BTreeSet<&Path> = entries.iter().filter_map(|entry| entry.parent()).collect();
    for folder in folders {
        match watcher.watch(folder) {
            // Not there yet: the folder above it is watched for it.
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                logger.warn(LOG_SOURCE, &format!("can't watch {} for settings changes: {e}", folder.display()));
            }
            _ => {}
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::LogLevel;

    /// A file changed between the first read and the watches going in is
    /// only seen if the watcher reads once more after they're in.
    #[test]
    fn reads_once_more_once_watching() {
        let dir = nova_test_support::TempDir::new("settings-watch");
        let logger =
            Arc::new(Logger::open(&dir.join("logs"), LogLevel::Info, false, Arc::new(Hub::default())).unwrap());
        let (reloaded, reloads) = mpsc::channel();
        let watching = watch(vec![dir.join("config.toml")], logger, move || {
            let _ = reloaded.send(());
        })
        .unwrap();
        assert!(reloads.recv_timeout(SETTLE * 10).is_ok(), "no reload after the watches went in");
        watching.stop();
    }
}
