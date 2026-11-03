//! Finding files by name (`files_search`), FSearch-style: novad keeps an
//! index of every name under Home and the drives, hidden folders included
//! (so a game in `~/.local/share/Steam` is found), and a search only reads
//! that, so it answers in milliseconds.
//!
//! One thread keeps the index. It reads every root into a new index
//! ([`walk`]), watching each folder with inotify as it goes, and swaps it
//! in; then it applies what inotify reports: names that came are added
//! (a folder with everything in it), names that went are removed. It
//! builds a new index from scratch every [`RESCAN_EVERY`], which also
//! catches folders it couldn't watch (past the system's watch limit) and
//! drops the space removed names left; and again, no sooner than
//! [`RESCAN_GAP`] after the last, when inotify lost events (a full queue)
//! or the mounts changed (a drive plugged in or out:
//! `/proc/self/mountinfo` wakes the thread).
//!
//! A search ([`Searcher::search`]) ranks the names under a read lock, then
//! looks at the files it returns (their type, size and date) after
//! letting go, so a slow drive never holds up the index. One that has
//! gone since is left out.
//!
//! - `index.rs`: the names, as a compact tree, and ranking.
//! - `matching.rs`: how a query matches a name, and where.
//! - `roots.rs`: which folders are searched, from the mounts.
//! - `walk.rs`: reading folders into the index, and what's left out.

mod index;
mod matching;
mod roots;
mod walk;

use crate::files::folder::read_entry;
use crate::files::views::checked_path;
use crate::hub::Hub;
use crate::inotify::{self, Changes, Inotify, Stopper};
use crate::logger::Logger;
use crate::mime::Mime;
use index::Index;
use matching::Query;
use nova_protocol::{
    Event, FILES_SEARCH_MAX_CHARS, FILES_SEARCH_MAX_RESULTS, FileSearch, FoundFile, wire_path, wire_text,
};
use roots::Mounts;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use walk::Walk;

const LOG_SOURCE: &str = "files";
/// How often the index is built anew.
const RESCAN_EVERY: Duration = Duration::from_secs(60 * 60);
/// The soonest a rescan follows the last, so a folder that changes faster
/// than inotify can report doesn't keep the disk busy.
const RESCAN_GAP: Duration = Duration::from_secs(60);
/// What each folder's watch reports: entries coming and going, and the
/// folder itself going.
const WATCHED: u32 = inotify::ENTRIES | inotify::ITSELF | libc::IN_ONLYDIR | libc::IN_EXCL_UNLINK;
const BYTES_PER_MB: usize = 1024 * 1024;

/// Where the search looks.
#[derive(Clone)]
pub struct Sources {
    pub home: Option<PathBuf>,
    /// The mounts, `/proc/self/mountinfo`.
    pub mountinfo: PathBuf,
    /// Where drives are mounted ([`roots::DRIVE_PLACES`]).
    pub drive_places: Vec<PathBuf>,
}

impl Sources {
    /// This machine's.
    pub fn system(home: Option<PathBuf>) -> Self {
        Self {
            home,
            mountinfo: "/proc/self/mountinfo".into(),
            drive_places: roots::DRIVE_PLACES.iter().map(PathBuf::from).collect(),
        }
    }

    /// For tests: Home is `<dir>/home`, the mounts are listed in
    /// `<dir>/mountinfo` (none while it's missing), and there are no
    /// drives.
    #[cfg(test)]
    pub fn under(dir: &Path) -> Self {
        Self { home: Some(dir.join("home")), mountinfo: dir.join("mountinfo"), drive_places: Vec::new() }
    }
}

/// Keeps the index while it runs.
pub struct Search {
    shared: Arc<Shared>,
    stopper: Option<Stopper>,
}

/// What searches read: the index, none until it's built.
struct Shared {
    index: RwLock<Option<Index>>,
    mime: Arc<Mime>,
}

/// Answers searches; cheap to clone into a request's own thread.
#[derive(Clone)]
pub struct Searcher(Arc<Shared>);

impl Search {
    /// Starts building the index of `sources`' folders.
    /// Says on `hub` when it's first built ([`Event::FilesSearchReady`]).
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>, mime: Arc<Mime>) -> Self {
        let shared = Arc::new(Shared { index: RwLock::new(None), mime });
        let started = Inotify::new(WATCHED).and_then(|watcher| {
            let stopper = watcher.stopper();
            let keeper = Keeper { sources, watcher, shared: Arc::clone(&shared), hub, logger: Arc::clone(&logger) };
            std::thread::Builder::new().name("novad-files-search".into()).spawn(move || keeper.run())?;
            Ok(stopper)
        });
        let stopper = started.inspect_err(|e| logger.error(LOG_SOURCE, &format!("Can't index files: {e}"))).ok();
        Self { shared, stopper }
    }

    pub fn searcher(&self) -> Searcher {
        Searcher(Arc::clone(&self.shared))
    }

    /// Stops keeping the index; searches find what it held.
    pub fn stop(&self) {
        if let Some(stopper) = &self.stopper {
            stopper.stop();
        }
    }
}

impl Searcher {
    /// The files whose names hold every word of `text`, below the folder
    /// `within` (absolute, wire text) when it's given. A folder the index
    /// doesn't hold as itself (a link to a folder, say `~/Downloads` to a
    /// drive) is searched where it leads.
    pub fn search(&self, text: &str, within: Option<&str>) -> Result<FileSearch, String> {
        if text.chars().count() > FILES_SEARCH_MAX_CHARS {
            return Err(format!("Searches are at most {FILES_SEARCH_MAX_CHARS} characters"));
        }
        let within = within.map(checked_path).transpose()?;
        // Read before taking the lock, so a slow drive never holds it.
        let leads_to = within.as_deref().and_then(|folder| std::fs::canonicalize(folder).ok());
        let held = self.0.index.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let ready = held.is_some();
        let (Some(index), Some(query)) = (held.as_ref(), Query::parse(text)) else {
            return Ok(FileSearch { ready, total: 0, results: Vec::new() });
        };
        let scope = match (within.as_deref(), leads_to.as_deref()) {
            (Some(folder), Some(target)) if !index.covers(folder) => Some(target),
            (folder, _) => folder,
        };
        let (hits, total) = index.search(&query, scope, FILES_SEARCH_MAX_RESULTS);
        drop(held);
        let mime = self.0.mime.current();
        let results = hits
            .into_iter()
            .filter_map(|hit| {
                let name = wire_path(&hit.name);
                let entry = read_entry(&wire_path(&hit.folder), name.as_os_str(), &mime)?;
                Some(FoundFile {
                    folder: hit.folder,
                    marks: query.marks(&entry.name),
                    entry,
                    in_hidden_folder: hit.in_hidden_folder,
                })
            })
            .collect();
        Ok(FileSearch { ready, total: u32::try_from(total).unwrap_or(u32::MAX), results })
    }
}

/// The thread that keeps the index.
struct Keeper {
    sources: Sources,
    watcher: Inotify,
    shared: Arc<Shared>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
}

/// What the index was built from, to follow its changes.
struct Built {
    mounts: Mounts,
    skipped: Vec<PathBuf>,
    /// The folders searched and the mounts inside them: when either
    /// changes, it's time for a rescan.
    reach: Reach,
    /// When it was done.
    at: Instant,
}

type Reach = (Vec<PathBuf>, Vec<(PathBuf, bool)>);

impl Keeper {
    fn run(mut self) {
        match std::fs::File::open(&self.sources.mountinfo) {
            Ok(mountinfo) => self.watcher.wake_on(Arc::new(mountinfo), libc::POLLPRI),
            Err(e) => {
                self.logger.warn(LOG_SOURCE, &format!("Can't follow mounts, so new drives wait for a rescan: {e}"))
            }
        }
        let stopper = self.watcher.stopper();
        while let Some(built) = self.build(&stopper) {
            if !self.follow(&built, &stopper) {
                return;
            }
        }
    }

    /// Builds the index anew and swaps it in; `None` once stopped.
    fn build(&mut self, stopper: &Stopper) -> Option<Built> {
        let started = Instant::now();
        let mounts = self.mounts();
        let reach = self.reach(&mounts);
        let roots = &reach.0;
        // Each root is read as its own, never as part of another.
        let skipped: Vec<PathBuf> =
            walk::skipped_in(self.sources.home.as_deref()).into_iter().chain(roots.iter().cloned()).collect();
        let mut index = Index::default();
        let mut walk =
            Walk { watcher: &mut self.watcher, mounts: &mounts, skipped: &skipped, stopper, unwatched: None };
        for root in roots {
            let id = index.add_root(root);
            if !walk.read(&mut index, id, root.clone()) {
                return None;
            }
        }
        index.shrink();
        if let Some(e) = walk.unwatched {
            self.logger.warn(
                LOG_SOURCE,
                &format!("Can't watch more folders ({e}): the rest of the search index is only updated by rescans"),
            );
        }
        self.logger.info(
            LOG_SOURCE,
            &format!(
                "Indexed {} names in {:.1}s ({} MB, {} folders watched)",
                index.len(),
                started.elapsed().as_secs_f32(),
                index.bytes() / BYTES_PER_MB,
                self.watcher.watched(),
            ),
        );
        let first =
            self.shared.index.write().unwrap_or_else(std::sync::PoisonError::into_inner).replace(index).is_none();
        if first {
            self.hub.publish(&Event::FilesSearchReady);
        }
        Some(Built { mounts, skipped, reach, at: Instant::now() })
    }

    /// Applies changes until a rescan is due; `false` once stopped.
    fn follow(&mut self, built: &Built, stopper: &Stopper) -> bool {
        let mut rescan_at = built.at + RESCAN_EVERY;
        loop {
            let changes = match self.watcher.read_until(Some(rescan_at)) {
                Ok(Some(changes)) => changes,
                Ok(None) => return false,
                Err(e) => {
                    self.logger.error(LOG_SOURCE, &format!("Stopped following file changes: {e}"));
                    return false;
                }
            };
            if Instant::now() >= rescan_at {
                return true;
            }
            if changes.overflowed || (changes.woken && self.reach(&self.mounts()) != built.reach) {
                rescan_at = rescan_at.min((built.at + RESCAN_GAP).max(Instant::now()));
            }
            self.apply(changes, built, stopper);
        }
    }

    /// Adds what came and removes what went.
    fn apply(&mut self, changes: Changes, built: &Built, stopper: &Stopper) {
        // Looked at before taking the lock. A folder that came is read
        // under it, which is quick for all but the biggest moved in.
        let found: Vec<(PathBuf, Option<bool>)> = changes
            .found
            .into_iter()
            .map(|change| {
                let dir = std::fs::symlink_metadata(&change.path).ok().map(|metadata| metadata.is_dir());
                (change.path, dir)
            })
            .collect();
        let gone: Vec<PathBuf> = changes.gone.into_iter().filter(|path| !path.exists()).collect();
        if found.is_empty() && gone.is_empty() {
            return;
        }
        let mut index = self.shared.index.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(index) = index.as_mut() else { return };
        let mut walk = Walk {
            watcher: &mut self.watcher,
            mounts: &built.mounts,
            skipped: &built.skipped,
            stopper,
            unwatched: None,
        };
        for path in gone {
            walk.forget(&path);
            if let Some(id) = index.find(&path) {
                index.remove(id);
            }
        }
        for (path, dir) in found {
            let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else { continue };
            let Some(parent) = index.find(folder) else {
                // A folder that left the index (moved to the trash) but is
                // still watched.
                walk.forget(folder);
                continue;
            };
            let name = wire_text(name);
            match dir {
                Some(dir) => {
                    // Read afresh: what was there by this name before may
                    // have been replaced since.
                    if let Some(old) = index.child(parent, &name) {
                        index.remove(old);
                        walk.forget(&path);
                    }
                    let id = index.add_new(parent, &name, dir);
                    if dir && walk.follows(&path) && !walk.read(index, id, path) {
                        return;
                    }
                }
                None => {
                    walk.forget(&path);
                    if let Some(id) = index.child(parent, &name) {
                        index.remove(id);
                    }
                }
            }
        }
    }

    /// The folders searched, given `mounts`, and the mounts inside them.
    fn reach(&self, mounts: &Mounts) -> Reach {
        let places: Vec<&Path> = self.sources.drive_places.iter().map(PathBuf::as_path).collect();
        let roots = mounts.roots(self.sources.home.as_deref(), &places);
        let inside = mounts.inside(&roots);
        (roots, inside)
    }

    /// The mounts now; none when they can't be read.
    fn mounts(&self) -> Mounts {
        match std::fs::read(&self.sources.mountinfo) {
            Ok(mountinfo) => Mounts::parse(&mountinfo),
            Err(_) => Mounts::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::{LogLevel, TextMark};
    use nova_test_support::{TempDir, wait_until};

    const WAIT: Duration = Duration::from_secs(5);

    /// A search over `<dir>/home`, running.
    struct Fixture {
        dir: TempDir,
        search: Search,
    }

    impl Fixture {
        fn new(name: &str, prepare: impl FnOnce(&Path)) -> Self {
            let dir = TempDir::new(&format!("files-search-{name}"));
            std::fs::create_dir_all(dir.join("home")).unwrap();
            prepare(&dir.join("home"));
            let hub = Arc::new(crate::hub::Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let mime = Mime::follow(Vec::new(), Arc::clone(&logger));
            let search = Search::start(Sources::under(&dir), hub, logger, mime);
            let fixture = Self { dir, search };
            wait_until(WAIT, "the index is built", || fixture.find("").ready);
            fixture
        }

        fn home(&self) -> PathBuf {
            self.dir.join("home")
        }

        fn find(&self, query: &str) -> FileSearch {
            self.search.searcher().search(query, None).unwrap()
        }

        /// The paths found, under Home.
        fn paths(&self, query: &str) -> Vec<String> {
            let home = self.home();
            let found = self.find(query).results.into_iter();
            found
                .map(|file| {
                    let path = Path::new(&file.folder).join(&file.entry.name);
                    path.strip_prefix(&home).unwrap().display().to_string()
                })
                .collect()
        }

        /// Waits until `query` finds `paths`.
        fn finds(&self, query: &str, paths: &[&str]) {
            wait_until(WAIT, &format!("{query:?} finds {paths:?}"), || self.paths(query) == paths);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.search.stop();
        }
    }

    fn steam_game(home: &Path) {
        let game = home.join(".local/share/Steam/steamapps/common/Crimson Desert");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("CrimsonDesert.exe"), b"exe").unwrap();
    }

    #[test]
    fn a_linked_folder_is_searched_where_it_leads() {
        let fixture = Fixture::new("link", |home| {
            std::fs::create_dir_all(home.join("data/downloads")).unwrap();
            std::fs::write(home.join("data/downloads/setup.sh"), b"").unwrap();
            std::os::unix::fs::symlink(home.join("data/downloads"), home.join("Downloads")).unwrap();
        });
        let link = fixture.home().join("Downloads");
        let found = fixture.search.searcher().search("setup", Some(link.to_str().unwrap())).unwrap();
        assert_eq!(found.results.iter().map(|file| file.entry.name.as_str()).collect::<Vec<_>>(), ["setup.sh"]);
        assert_eq!(found.results[0].folder, fixture.home().join("data/downloads").to_str().unwrap());
    }

    #[test]
    fn a_game_in_a_hidden_folder_is_found_with_its_details() {
        let fixture = Fixture::new("steam", steam_game);
        let found = fixture.find("crimson DESERT");
        assert_eq!((found.ready, found.total), (true, 2));
        let exe = &found.results[1];
        assert_eq!(exe.entry.name, "CrimsonDesert.exe");
        assert_eq!(exe.entry.size, Some(3));
        assert!(exe.in_hidden_folder && !exe.entry.hidden);
        assert_eq!(exe.marks, [TextMark { start: 0, length: 7 }, TextMark { start: 7, length: 6 }]);
        assert!(found.results[0].entry.dir);
        assert!(fixture.find("   ").results.is_empty());
        assert!(fixture.search.searcher().search(&"x".repeat(FILES_SEARCH_MAX_CHARS + 1), None).is_err());
        assert!(fixture.search.searcher().search("x", Some("relative/folder")).is_err());
    }

    #[test]
    fn names_that_come_and_go_are_followed() {
        let fixture = Fixture::new("follow", steam_game);
        let home = fixture.home();
        std::fs::write(home.join("crimson notes.txt"), b"").unwrap();
        fixture.finds("notes", &["crimson notes.txt"]);
        // A folder made with things already in it, like a download unpacked.
        let unpacked = home.join("unpack-tmp");
        std::fs::create_dir_all(unpacked.join("inner")).unwrap();
        std::fs::write(unpacked.join("inner/manual.pdf"), b"").unwrap();
        std::fs::rename(&unpacked, home.join("Manuals")).unwrap();
        fixture.finds("manual", &["Manuals", "Manuals/inner/manual.pdf"]);
        // A folder renamed takes its names with it.
        std::fs::rename(home.join("Manuals"), home.join("Docs")).unwrap();
        fixture.finds("manual", &["Docs/inner/manual.pdf"]);
        std::fs::write(home.join("Docs/inner/manual 2.pdf"), b"").unwrap();
        fixture.finds("manual", &["Docs/inner/manual.pdf", "Docs/inner/manual 2.pdf"]);
        std::fs::remove_dir_all(home.join(".local/share/Steam")).unwrap();
        fixture.finds("crimson", &["crimson notes.txt"]);
    }

    #[test]
    fn a_folder_that_leaves_takes_its_watches_with_it() {
        let fixture =
            Fixture::new("unwatch", |home| std::fs::create_dir_all(home.join("Projects/app/src/deep")).unwrap());
        let home = fixture.home();
        let outside = fixture.dir.join("outside");
        std::fs::rename(home.join("Projects/app"), &outside).unwrap();
        fixture.finds("deep", &[]);
        // Something written in the old tree now isn't reported under its old path.
        std::fs::create_dir(home.join("Projects/app")).unwrap();
        std::fs::write(outside.join("src/deep/stray.txt"), b"").unwrap();
        std::fs::write(home.join("Projects/app/fresh.txt"), b"").unwrap();
        fixture.finds("fresh", &["Projects/app/fresh.txt"]);
        assert!(fixture.paths("stray").is_empty());
    }

    #[test]
    fn a_folder_replaced_by_one_of_the_same_name_loses_its_old_names() {
        let fixture = Fixture::new("replaced", |home| {
            std::fs::create_dir_all(home.join("dist")).unwrap();
            std::fs::write(home.join("dist/old-bundle.js"), b"").unwrap();
        });
        let home = fixture.home();
        std::fs::create_dir_all(home.join("dist-new")).unwrap();
        std::fs::write(home.join("dist-new/new-bundle.js"), b"").unwrap();
        std::fs::remove_dir_all(home.join("dist")).unwrap();
        std::fs::rename(home.join("dist-new"), home.join("dist")).unwrap();
        fixture.finds("bundle", &["dist/new-bundle.js"]);
    }

    #[test]
    fn a_name_that_is_not_utf8_is_found_and_names_its_file() {
        use std::os::unix::ffi::OsStrExt;
        let latin1 = std::ffi::OsStr::from_bytes(b"Caf\xe9");
        let fixture = Fixture::new("latin1", |home| {
            std::fs::create_dir_all(home.join(latin1)).unwrap();
            std::fs::write(home.join(latin1).join("menu.pdf"), b"").unwrap();
        });
        let found = fixture.find("menu");
        assert_eq!(found.results.len(), 1, "the folder was walked");
        assert_eq!(wire_path(&found.results[0].folder), fixture.home().join(latin1));
        assert_eq!(fixture.find("caf").results[0].entry.name, wire_text(latin1));
    }

    #[test]
    fn a_file_gone_before_it_is_looked_at_is_left_out() {
        let fixture = Fixture::new("gone", |home| std::fs::write(home.join("report.pdf"), b"").unwrap());
        fixture.search.stop();
        std::fs::remove_file(fixture.home().join("report.pdf")).unwrap();
        let found = fixture.find("report");
        assert_eq!((found.total, found.results.len()), (1, 0));
    }
}
