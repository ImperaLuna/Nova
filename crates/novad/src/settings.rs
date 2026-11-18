//! The user's settings, `config.toml` and `features.toml`: read at start and
//! again whenever either file changes, so edits apply without a restart.
//! Both hand the features' states and the config to the module registry
//! (`crate::modules`), whose one loop starts, stops and reconfigures
//! modules, so a reload ends where a fresh start with the same files would.
//!
//! A file that doesn't parse means defaults at start, and keeps the last good
//! version after that, so a half-finished edit never blanks the UI. Either
//! way the problem is logged.

use crate::compositor::Compositor;
use crate::config::{self, Config};
use crate::hub::Hub;
use crate::icons::Icons;
use crate::inotify::{self, Inotify};
use crate::logger::Logger;
use crate::modules::{self, Context, Running};
use crate::{apps, features, host, theme};
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
pub(crate) const SETTLE: Duration = Duration::from_millis(200);

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
        let features = features::states(&sources.manifests, &selection, &sources.path_var);
        let theme = resolve_theme(sources, &config, logger);
        hub.publish(&Event::Theme(theme.clone()));
        let context = Context {
            hub: Arc::clone(hub),
            logger: Arc::clone(logger),
            path_var: sources.path_var.clone(),
            icons: Arc::clone(&sources.icons),
            compositor,
            apps: apps::AppIndex::new(sources.modules.apps.clone(), &sources.path_var, Arc::clone(logger)),
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
        let features = features::states(&sources.manifests, &selection, &sources.path_var);
        if self.config.theme != config.theme {
            let theme = resolve_theme(sources, &config, logger);
            if theme != self.theme {
                hub.publish(&Event::Theme(theme.clone()));
            }
            self.theme = theme;
        }
        if self.config.quick_settings != config.quick_settings {
            hub.publish(&Event::Host(host::read(config.quick_settings.name.as_deref(), &sources.icons)));
        }
        self.modules.apply(&self.context, &sources.modules, &config, &features);
        publish_features(&self.features, &features, hub, logger);
        self.config = config;
        self.selection = selection;
        self.features = features;
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
        host::read(self.config.quick_settings.name.as_deref(), &self.context.icons)
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

/// The theme `config` picks, its problems logged.
fn resolve_theme(sources: &Sources, config: &Config, logger: &Logger) -> Theme {
    let (theme, problems) = theme::resolve(&sources.themes_dir, &config.theme);
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
/// and returns those entries: each file, every link on the way to what it
/// reads (a dotfiles checkout, say, or NixOS's `/etc/static`, which a
/// rebuild replaces while `/etc/xdg/nova/features.toml` stays the same
/// link), each path those links lead to, and their folders, which may
/// appear later, be replaced or be links themselves. Watching a folder
/// that's already watched is harmless.
fn watch_entries(watcher: &mut Inotify, files: &[PathBuf], logger: &Logger) -> BTreeSet<PathBuf> {
    let mut entries = BTreeSet::new();
    for file in files {
        for entry in link_hops(file) {
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

/// Links followed before giving up, as the kernel's own limit.
const MAX_LINK_HOPS: usize = 40;

/// `file`, and each link met while resolving it, from the root down, with
/// the path it leads to: for `/etc/xdg/nova/features.toml` on NixOS, also
/// `/etc/static` and the store path behind it.
fn link_hops(file: &Path) -> Vec<PathBuf> {
    let mut hops = vec![file.to_owned()];
    let mut path = file.to_owned();
    for _ in 0..MAX_LINK_HOPS {
        let ancestors: Vec<&Path> = path.ancestors().collect();
        let Some((link, target)) = ancestors
            .into_iter()
            .rev()
            .find_map(|ancestor| std::fs::read_link(ancestor).ok().map(|target| (ancestor.to_owned(), target)))
        else {
            break;
        };
        let rest = path.strip_prefix(&link).map(Path::to_owned).unwrap_or_default();
        let base = link.parent().unwrap_or(Path::new("/"));
        path = base.join(target).join(rest);
        hops.push(link);
        hops.push(path.clone());
    }
    hops
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::LogLevel;

    /// NixOS's layout: a link to `/etc/static/...`, where `static` is the
    /// link a rebuild replaces.
    #[test]
    fn every_link_on_the_way_is_a_hop() {
        let dir = nova_test_support::TempDir::new("settings-hops");
        std::fs::create_dir_all(dir.join("store/xdg/nova")).unwrap();
        std::fs::write(dir.join("store/xdg/nova/features.toml"), "").unwrap();
        std::os::unix::fs::symlink(dir.join("store"), dir.join("static")).unwrap();
        std::fs::create_dir_all(dir.join("etc/xdg/nova")).unwrap();
        std::os::unix::fs::symlink(dir.join("static/xdg/nova/features.toml"), dir.join("etc/xdg/nova/features.toml"))
            .unwrap();
        let file = dir.join("etc/xdg/nova/features.toml");
        let hops = link_hops(&file);
        assert!(hops.contains(&dir.join("static")), "{hops:?}");
        assert_eq!(hops.last(), Some(&dir.join("store/xdg/nova/features.toml")));
    }

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
