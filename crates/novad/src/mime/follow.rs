//! The MIME database as it is now ([`Mime::current`]), for Nova Files'
//! folder listing and opening files. It's read as the files module starts
//! and again, on a thread of its own, once `update-mime-database` rewrites
//! a `mime/` folder (a package that brings new types) or a rebuild swaps a
//! link on the way to one ([`crate::watch_points`]). Only each folder's own
//! files are watched: the database is those, the per-type XML below is
//! what they're built from.
//!
//! Whoever asks gets the copy read last; one read meanwhile replaces it
//! only once it's whole, so nobody sees half a database.

use super::MimeDb;
use crate::inotify::{self, Inotify};
use crate::lock;
use crate::logger::Logger;
use crate::watch_points::{self, Reporting, WatchPoints};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

const LOG_SOURCE: &str = "mime";
/// The database's folder in each data folder.
const FOLDER: &str = "mime";

pub struct Mime {
    current: Mutex<Arc<MimeDb>>,
    /// Ends the watching; `None` when it couldn't start.
    stopper: Option<inotify::Stopper>,
}

impl Mime {
    /// Reads the database in `data_dirs` (the most important first) and
    /// follows changes to it until stopped (or dropped).
    pub fn follow(data_dirs: Vec<PathBuf>, logger: Arc<Logger>) -> Arc<Self> {
        let (changed, changes) = mpsc::channel();
        let stopper = match Inotify::new(inotify::REPLACEMENTS) {
            Ok(watcher) => {
                let stopper = watcher.stopper();
                let dirs = data_dirs.clone();
                let reporting = Reporting { logger: Arc::clone(&logger), source: LOG_SOURCE, what: "new file types" };
                watch_points::follow(watcher, move || watch_points(&dirs), changed, reporting);
                Some(stopper)
            }
            Err(e) => {
                logger
                    .warn(LOG_SOURCE, &format!("can't watch for new file types ({e}); they show after `nova restart`"));
                None
            }
        };
        // Watched first, so types installed meanwhile aren't missed.
        let mime = Arc::new(Self { current: Mutex::new(Arc::new(load(&data_dirs, &logger))), stopper });
        let following = Arc::downgrade(&mime);
        std::thread::spawn(move || {
            // Ends once the watching does: it holds the only sender.
            while changes.recv().is_ok() {
                while changes.recv_timeout(watch_points::SETTLE).is_ok() {}
                let Some(mime) = following.upgrade() else { return };
                if mime.stopped() {
                    return;
                }
                let db = load(&data_dirs, &logger);
                if mime.stopped() {
                    return;
                }
                *lock(&mime.current) = Arc::new(db);
                logger.info(LOG_SOURCE, "read the MIME database again");
            }
        });
        mime
    }

    /// The database as last read.
    pub fn current(&self) -> Arc<MimeDb> {
        Arc::clone(&lock(&self.current))
    }

    /// Stops following changes; what it read last stays.
    pub fn stop(&self) {
        if let Some(stopper) = &self.stopper {
            stopper.stop();
        }
    }

    fn stopped(&self) -> bool {
        self.stopper.as_ref().is_some_and(inotify::Stopper::stopped)
    }
}

impl Drop for Mime {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Where to watch for the database in `data_dirs`.
fn watch_points(data_dirs: &[PathBuf]) -> WatchPoints {
    let mut points = WatchPoints::default();
    for dir in data_dirs {
        points.add_folder(&dir.join(FOLDER));
    }
    points
}

fn load(data_dirs: &[PathBuf], logger: &Logger) -> MimeDb {
    let (db, problems) = MimeDb::load(data_dirs);
    for problem in problems {
        logger.warn(LOG_SOURCE, &problem);
    }
    db
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::LogLevel;
    use nova_test_support::{TempDir, wait_until};
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(5);

    fn logger(dir: &TempDir) -> Arc<Logger> {
        Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::default()).unwrap())
    }

    fn type_of(mime: &Mime, file: &std::path::Path) -> String {
        mime.current().type_of(file).unwrap()
    }

    /// Like `update-mime-database`: the new file written beside the old,
    /// then renamed over it.
    fn rewrite(dir: &std::path::Path, name: &str, text: &str) {
        let new = dir.join(format!("{name}.new"));
        std::fs::write(&new, text).unwrap();
        std::fs::rename(new, dir.join(name)).unwrap();
    }

    #[test]
    fn a_type_installed_later_is_known_once_written() {
        let dir = super::super::tests::data_dir("mime-follow-installed");
        let mime = Mime::follow(vec![dir.to_path_buf()], logger(&dir));
        let file = dir.join("scene.blend");
        std::fs::write(&file, b"\x00\x01").unwrap();
        assert_eq!(type_of(&mime, &file), super::super::BINARY);
        let globs = std::fs::read_to_string(dir.join("mime/globs2")).unwrap();
        rewrite(&dir.join("mime"), "globs2", &format!("{globs}50:application/x-blender:*.blend\n"));
        wait_until(WAIT, "the new type", || type_of(&mime, &file) == "application/x-blender");
    }

    #[test]
    fn a_database_made_after_it_started_is_found() {
        let dir = TempDir::new("mime-follow-new-folder");
        let share = dir.join("share");
        std::fs::create_dir_all(&share).unwrap();
        let mime = Mime::follow(vec![share.clone()], logger(&dir));
        let file = dir.join("photo.png");
        std::fs::write(&file, b"\x00").unwrap();
        assert_eq!(type_of(&mime, &file), super::super::BINARY);
        // A Nix profile switch: a new store folder, then the link to it.
        let store = dir.join("store/mime");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("globs2"), "50:image/png:*.png\n").unwrap();
        std::os::unix::fs::symlink(&store, share.join("mime")).unwrap();
        wait_until(WAIT, "the linked database", || type_of(&mime, &file) == "image/png");
    }

    #[test]
    fn once_stopped_it_keeps_what_it_read() {
        let dir = super::super::tests::data_dir("mime-follow-stopped");
        let mime = Mime::follow(vec![dir.to_path_buf()], logger(&dir));
        let file = dir.join("photo.png");
        std::fs::write(&file, b"x").unwrap();
        mime.stop();
        rewrite(&dir.join("mime"), "globs2", "50:image/x-other:*.png\n");
        std::thread::sleep(watch_points::SETTLE * 2);
        assert_eq!(type_of(&mime, &file), "image/png");
    }
}
