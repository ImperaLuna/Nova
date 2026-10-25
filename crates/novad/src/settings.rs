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
use crate::inotify::{self, Inotify};
use crate::logger::Logger;
use crate::{ai_usage, features, launcher, theme};
use nova_features::{Manifest, Selection};
use nova_protocol::{Event, FeatureState, FeatureStatus, Theme};
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
    pub launcher: launcher::Paths,
}

/// The settings in effect, and the feature services they started.
pub struct Settings {
    config: Config,
    selection: Selection,
    pub features: Vec<FeatureState>,
    pub theme: Theme,
    /// Running while the `ai-usage` feature is ready.
    pub ai_usage: Option<Arc<ai_usage::Usage>>,
    /// Running while the `launcher` feature is ready.
    pub launcher: Option<Arc<launcher::Launcher>>,
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

        // Started before the features are published, so the UI that loads
        // the feature on that event gets its numbers.
        let ai_usage_ready = is_ready(&features, ai_usage::FEATURE_ID);
        let ai_usage_settings_kept =
            previous.as_deref().is_some_and(|previous| previous.config.ai_usage == config.ai_usage);
        let ai_usage = match previous.as_deref_mut().and_then(|previous| previous.ai_usage.take()) {
            Some(running) if ai_usage_ready && ai_usage_settings_kept => Some(running),
            running => {
                if let Some(running) = running {
                    running.stop();
                    logger.info("features", &format!("stopped {}", ai_usage::FEATURE_ID));
                }
                ai_usage_ready.then(|| {
                    logger.info("features", &format!("starting {}", ai_usage::FEATURE_ID));
                    ai_usage::Usage::start(
                        sources.ai_usage.clone(),
                        &config.ai_usage,
                        Arc::clone(hub),
                        Arc::clone(logger),
                    )
                })
            }
        };

        // It has no options: it only starts and stops with its feature.
        let launcher_ready = is_ready(&features, launcher::FEATURE_ID);
        let launcher = match previous.as_deref_mut().and_then(|previous| previous.launcher.take()) {
            Some(running) if launcher_ready => Some(running),
            running => {
                if let Some(running) = running {
                    running.stop();
                    logger.info("features", &format!("stopped {}", launcher::FEATURE_ID));
                }
                launcher_ready.then(|| {
                    logger.info("features", &format!("starting {}", launcher::FEATURE_ID));
                    launcher::Launcher::start(
                        sources.launcher.clone(),
                        &sources.path_var,
                        Arc::clone(hub),
                        Arc::clone(logger),
                    )
                })
            }
        };

        let before = previous.as_deref().map(|previous| previous.features.as_slice()).unwrap_or_default();
        for feature in features.iter().filter(|feature| !before.contains(feature)) {
            logger.info("features", &format!("{} is {:?}", feature.id, feature.status));
        }
        if before != features {
            hub.publish(&Event::Features { features: features.clone() });
        }

        Self { config, selection, features, theme, ai_usage, launcher }
    }
}

fn is_ready(features: &[FeatureState], id: &str) -> bool {
    features.iter().any(|feature| feature.id == id && feature.status == FeatureStatus::Ready)
}

/// `read`'s settings, or `None` after logging why not and what happens
/// instead. The outcome goes first: a TOML error spans several lines.
fn logged<T>(read: Result<T, String>, fallback: &str, logger: &Logger) -> Option<T> {
    read.map_err(|problem| logger.error(LOG_SOURCE, &format!("{fallback}: {problem}"))).ok()
}

/// Calls `reload` on a thread of its own once `files` stop changing. The
/// folders around them are watched rather than the files, so a file that's
/// replaced (saved by rename, a new symlink) is still seen, and so is a
/// folder that appears later or is replaced.
pub fn watch(files: Vec<PathBuf>, logger: Arc<Logger>, reload: impl Fn() + Send + 'static) {
    let mut watcher = match Inotify::new(inotify::REPLACEMENTS) {
        Ok(watcher) => watcher,
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("can't watch the settings ({e}); changes apply after `nova restart`"));
            return;
        }
    };
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
            while changes.recv_timeout(SETTLE).is_ok() {}
            reload();
        }
    });
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
        let dir = std::env::temp_dir().join(format!("novad-settings-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let logger =
            Arc::new(Logger::open(&dir.join("logs"), LogLevel::Info, false, Arc::new(Hub::default())).unwrap());
        let (reloaded, reloads) = mpsc::channel();
        watch(vec![dir.join("config.toml")], logger, move || {
            let _ = reloaded.send(());
        });
        assert!(reloads.recv_timeout(SETTLE * 10).is_ok(), "no reload after the watches went in");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
