use nova_inotify::{self as inotify, Inotify};
use nova_log::Logger;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

const LOG_SOURCE: &str = "settings";
pub const SETTLE: Duration = Duration::from_millis(200);

pub fn read_toml<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn user_path(setting: &str, path: &str) -> Result<PathBuf, String> {
    let resolved = match path.strip_prefix("~/") {
        Some(rest) => nova_paths::home().ok_or("HOME isn't set, so ~ means nothing")?.join(rest),
        None => PathBuf::from(path),
    };
    match resolved.is_absolute() {
        true => Ok(resolved),
        false => Err(format!("{setting} {path:?} isn't an absolute path or one under ~/")),
    }
}

pub fn folder(section: &str, configured: Option<&str>, default: impl FnOnce() -> PathBuf) -> Result<PathBuf, String> {
    match configured {
        Some(folder) => user_path(&format!("[{section}] folder"), folder),
        None => Ok(default()),
    }
}

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
            entries = watch_entries(&mut watcher, &files, &watch_logger);
            if changed.send(()).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        while changes.recv().is_ok() {
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
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                logger.warn(LOG_SOURCE, &format!("can't watch {} for settings changes: {e}", folder.display()));
            }
            _ => {}
        }
    }
    entries
}

// The kernel's own limit on links followed: https://man7.org/linux/man-pages/man7/path_resolution.7.html
const MAX_LINK_HOPS: usize = 40;

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
    use nova_carrier::log::LogLevel;
    use nova_hub::Hub;

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
