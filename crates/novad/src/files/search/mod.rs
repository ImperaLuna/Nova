//! Finding files by name (`files_search`), FSearch-style: novad keeps an
//! index of every name under Home and the drives, hidden folders included
//! (so a game in `~/.local/share/Steam` is found), and a search only reads
//! that, so it answers in milliseconds.
//!
//! The index is kept only while it's used: novad's Pss grows by about
//! 110 MB over a big home, and its inotify watches count against the
//! user's limit. A Nova Files process attached or a launcher open uses it
//! ([`Search::set_used`], by connection, so one that hangs up stops); the
//! first has it built (about 2 s over a home of 1.5M names), and it's
//! dropped with its watches [`KEPT_UNUSED`] after the last goes. A search
//! with nobody using it has it built and kept as long too. One asked for
//! while it builds answers `ready: false`, as before the first build.
//!
//! One thread keeps the index while it's used. It reads every root into
//! a new index ([`walk`]), watching each folder with inotify as it goes,
//! and swaps it in; then it applies what inotify reports: names that came are added
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
use crate::hub::ClientId;
use crate::inotify::{self, Changes, Inotify, Stopper};
use crate::lock;
use crate::logger::Logger;
use crate::mime::Mime;
use index::Index;
use matching::Query;
use nova_protocol::{FILES_SEARCH_MAX_CHARS, FILES_SEARCH_MAX_RESULTS, FileSearch, FoundFile, wire_path, wire_text};
use roots::Mounts;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
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
/// How long the index is kept once nothing uses it, so a launcher or Nova
/// Files opened again soon finds it built rather than building it again.
const KEPT_UNUSED: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(3 * 60) };

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

/// Who uses the index: a Nova Files process attached, or a launcher open,
/// each by its connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Use {
    Files,
    Launcher,
}

/// Keeps the index while it's used, and a while after.
pub struct Search {
    shared: Arc<Shared>,
}

/// What searches read, and who uses it.
struct Shared {
    index: RwLock<Held>,
    mime: Arc<Mime>,
    sources: Sources,
    logger: Arc<Logger>,
    demand: Mutex<Demand>,
    /// Wakes the thread that drops the index when [`Shared::demand`] changes.
    changed: Condvar,
    /// Woken each time an index is swapped in, for searches that wait for
    /// one ([`Searcher::wait_ready`]); the lock is taken after the swap.
    built: (Mutex<()>, Condvar),
}

/// The index, none until it's built and after it's dropped.
#[derive(Default)]
struct Held {
    index: Option<Index>,
    /// Which keeper it's from: one that was stopped never writes over its
    /// successor's.
    generation: u64,
}

/// Who uses the index, under one lock with starting and stopping its keeper.
#[derive(Default)]
struct Demand {
    users: HashSet<(ClientId, Use)>,
    /// Nobody uses it: it's dropped then, unless someone does by then.
    drop_at: Option<Instant>,
    /// Stops the thread keeping the index, while one runs.
    keeper: Option<Stopper>,
    generation: u64,
    /// [`Search::stop`]ped: no keeper starts again.
    ended: bool,
}

/// Answers searches; cheap to clone into a request's own thread.
#[derive(Clone)]
pub struct Searcher(Arc<Shared>);

impl Search {
    /// Ready to index `sources`' folders once something uses them.
    pub fn start(sources: Sources, logger: Arc<Logger>, mime: Arc<Mime>) -> Self {
        let shared = Arc::new(Shared {
            index: RwLock::default(),
            mime,
            sources,
            logger,
            demand: Mutex::default(),
            changed: Condvar::new(),
            built: (Mutex::new(()), Condvar::new()),
        });
        let dropper = Arc::clone(&shared);
        let spawned =
            std::thread::Builder::new().name("novad-files-index-idle".into()).spawn(move || dropper.drop_unused());
        if let Err(e) = spawned {
            shared.logger.warn(LOG_SOURCE, &format!("The search index won't be dropped while unused: {e}"));
        }
        Self { shared }
    }

    pub fn searcher(&self) -> Searcher {
        Searcher(Arc::clone(&self.shared))
    }

    /// Client `client` starts or stops using the index for `what`. The
    /// first user has it built; it's dropped [`KEPT_UNUSED`] after the last
    /// one goes.
    pub fn set_used(&self, client: ClientId, what: Use, used: bool) {
        let mut demand = lock(&self.shared.demand);
        let was_used = !demand.users.is_empty();
        if used {
            demand.users.insert((client, what));
        } else {
            demand.users.remove(&(client, what));
        }
        self.shared.used(&mut demand, was_used);
    }

    /// The client `client` hung up: it uses nothing any more.
    pub fn client_gone(&self, client: ClientId) {
        let mut demand = lock(&self.shared.demand);
        let was_used = !demand.users.is_empty();
        demand.users.retain(|(user, _)| *user != client);
        self.shared.used(&mut demand, was_used);
    }

    /// Stops keeping the index for good; searches find what it held.
    pub fn stop(&self) {
        let mut demand = lock(&self.shared.demand);
        demand.ended = true;
        if let Some(keeper) = demand.keeper.take() {
            keeper.stop();
        }
        self.shared.changed.notify_all();
    }

    /// The index is built now. Unlike a search, asking doesn't build it.
    #[cfg(test)]
    fn built(&self) -> bool {
        self.shared.is_built()
    }
}

impl Shared {
    fn is_built(&self) -> bool {
        self.index.read().unwrap_or_else(std::sync::PoisonError::into_inner).index.is_some()
    }

    /// A search asks for the index: with nobody using it, it's built and
    /// kept for [`KEPT_UNUSED`].
    fn asked(self: &Arc<Self>) {
        let mut demand = lock(&self.demand);
        if demand.users.is_empty() {
            self.build(&mut demand);
            demand.drop_at = Some(Instant::now() + KEPT_UNUSED);
            self.changed.notify_all();
        }
    }

    /// Who uses the index may have changed: it's built if anyone does,
    /// and set to be dropped once the last user goes. With nobody using it
    /// before either, the time it's dropped stays, so connections that
    /// never used it don't keep it by hanging up.
    fn used(self: &Arc<Self>, demand: &mut Demand, was_used: bool) {
        if demand.users.is_empty() {
            if was_used && demand.keeper.is_some() {
                demand.drop_at = Some(Instant::now() + KEPT_UNUSED);
            }
        } else {
            demand.drop_at = None;
            self.build(demand);
        }
        self.changed.notify_all();
    }

    /// Starts a keeper unless one runs.
    fn build(self: &Arc<Self>, demand: &mut Demand) {
        if demand.keeper.is_some() || demand.ended {
            return;
        }
        demand.generation += 1;
        let generation = demand.generation;
        self.index.write().unwrap_or_else(std::sync::PoisonError::into_inner).generation = generation;
        let started = Inotify::new(WATCHED).and_then(|watcher| {
            let stopper = watcher.stopper();
            let keeper = Keeper {
                sources: self.sources.clone(),
                watcher,
                shared: Arc::clone(self),
                logger: Arc::clone(&self.logger),
                generation,
            };
            std::thread::Builder::new().name("novad-files-search".into()).spawn(move || keeper.run())?;
            Ok(stopper)
        });
        match started {
            Ok(stopper) => demand.keeper = Some(stopper),
            Err(e) => self.logger.error(LOG_SOURCE, &format!("Can't index files: {e}")),
        }
    }

    /// The thread that drops the index once it's been unused for
    /// [`KEPT_UNUSED`], until the search stops.
    fn drop_unused(self: Arc<Self>) {
        let mut demand = lock(&self.demand);
        while !demand.ended {
            let Some(drop_at) = demand.drop_at else {
                demand = self.changed.wait(demand).unwrap_or_else(std::sync::PoisonError::into_inner);
                continue;
            };
            let now = Instant::now();
            if now < drop_at {
                demand = self
                    .changed
                    .wait_timeout(demand, drop_at - now)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
                continue;
            }
            demand.drop_at = None;
            if let Some(keeper) = demand.keeper.take() {
                keeper.stop();
            }
            demand.generation += 1;
            *self.index.write().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Held { index: None, generation: demand.generation };
            self.logger.info(LOG_SOURCE, &format!("Dropped the search index, unused for {} s", KEPT_UNUSED.as_secs()));
        }
    }
}

impl Searcher {
    /// Waits up to `timeout` for the index to be built; `true` once it is.
    /// With nobody using the index, it's built and kept as for a search.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        self.0.asked();
        let (lock, built) = &self.0.built;
        let held = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_held, _) = built
            .wait_timeout_while(held, timeout, |()| !self.0.is_built())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.0.is_built()
    }

    /// The files whose names hold every word of `text`. With nobody using
    /// the index, a search has it built and kept for [`KEPT_UNUSED`].
    pub fn search(&self, text: &str) -> Result<FileSearch, String> {
        self.search_up_to(text, FILES_SEARCH_MAX_RESULTS)
    }

    /// The best `limit` of them.
    pub fn search_up_to(&self, text: &str, limit: usize) -> Result<FileSearch, String> {
        if text.chars().count() > FILES_SEARCH_MAX_CHARS {
            return Err(format!("Searches are at most {FILES_SEARCH_MAX_CHARS} characters"));
        }
        self.0.asked();
        let held = self.0.index.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let ready = held.index.is_some();
        let (Some(index), Some(query)) = (held.index.as_ref(), Query::parse(text)) else {
            return Ok(FileSearch { ready, total: 0, results: Vec::new() });
        };
        let (hits, total) = index.search(&query, limit);
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
    logger: Arc<Logger>,
    /// Its own: it writes the index only while it's the latest.
    generation: u64,
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
        let mut held = self.shared.index.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.generation != self.generation {
            return None;
        }
        held.index = Some(index);
        drop(held);
        let (lock, built) = &self.shared.built;
        drop(lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        built.notify_all();
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
        let mut held = self.shared.index.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.generation != self.generation {
            return;
        }
        let Some(index) = held.index.as_mut() else { return };
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

    /// The connection Nova Files is on.
    const FILES: ClientId = 1;
    /// The UI's, with the launchers.
    const UI: ClientId = 2;

    impl Fixture {
        /// With the index built, and used by Nova Files so it's kept.
        fn new(name: &str, prepare: impl FnOnce(&Path)) -> Self {
            let fixture = Self::unused(name, prepare);
            fixture.search.set_used(FILES, Use::Files, true);
            fixture.wait_built(true);
            fixture
        }

        /// With nothing using the index yet.
        fn unused(name: &str, prepare: impl FnOnce(&Path)) -> Self {
            let dir = TempDir::new(&format!("files-search-{name}"));
            std::fs::create_dir_all(dir.join("home")).unwrap();
            prepare(&dir.join("home"));
            let hub = Arc::new(crate::hub::Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, hub).unwrap());
            let mime = Mime::follow(Vec::new(), Arc::clone(&logger));
            let search = Search::start(Sources::under(&dir), logger, mime);
            Self { dir, search }
        }

        /// Waits until the index is built, or until it's dropped.
        fn wait_built(&self, built: bool) {
            let what = if built { "the index is built" } else { "the index is dropped" };
            wait_until(WAIT, what, || self.search.built() == built);
        }

        fn home(&self) -> PathBuf {
            self.dir.join("home")
        }

        fn find(&self, query: &str) -> FileSearch {
            self.search.searcher().search(query).unwrap()
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
    fn a_search_can_wait_for_the_index_to_be_built() {
        let dir = TempDir::new("files-search-wait");
        std::fs::create_dir_all(dir.join("home/Games")).unwrap();
        let hub = Arc::new(crate::hub::Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, hub).unwrap());
        let mime = Mime::follow(Vec::new(), Arc::clone(&logger));
        let search = Search::start(Sources::under(&dir), logger, mime);
        // Nothing uses it yet: waiting has it built.
        assert!(search.searcher().wait_ready(WAIT));
        assert_eq!(search.searcher().search("games").unwrap().total, 1);
        search.stop();
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
        assert!(fixture.search.searcher().search(&"x".repeat(FILES_SEARCH_MAX_CHARS + 1)).is_err());
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
    fn the_index_is_built_while_a_launcher_is_open_and_dropped_after() {
        let fixture = Fixture::unused("launcher", steam_game);
        std::thread::sleep(KEPT_UNUSED);
        assert!(!fixture.search.built(), "nothing used it");
        fixture.search.set_used(UI, Use::Launcher, true);
        fixture.wait_built(true);
        // Open past the time an unused one is kept.
        std::thread::sleep(KEPT_UNUSED * 2);
        assert!(fixture.search.built());
        fixture.search.set_used(UI, Use::Launcher, false);
        assert!(fixture.search.built(), "kept a while after");
        fixture.wait_built(false);
        // Opened again: built afresh, and following changes again.
        fixture.search.set_used(UI, Use::Launcher, true);
        fixture.wait_built(true);
        std::fs::write(fixture.home().join("crimson notes.txt"), b"").unwrap();
        fixture.finds("notes", &["crimson notes.txt"]);
    }

    #[test]
    fn it_is_kept_until_its_last_user_goes() {
        let fixture = Fixture::new("users", steam_game);
        fixture.search.set_used(UI, Use::Launcher, true);
        // Nova Files hangs up, the launcher is still open.
        fixture.search.client_gone(FILES);
        std::thread::sleep(KEPT_UNUSED * 2);
        assert!(fixture.search.built());
        // The UI hanging up closes its launchers.
        fixture.search.client_gone(UI);
        fixture.wait_built(false);
    }

    #[test]
    fn connections_that_never_used_it_do_not_keep_it_by_hanging_up() {
        let fixture = Fixture::new("others", steam_game);
        fixture.search.set_used(FILES, Use::Files, false);
        // Short-lived clients, like `nova` commands, come and go.
        let other: ClientId = 100;
        wait_until(WAIT, "the index is dropped", || {
            fixture.search.client_gone(other);
            std::thread::sleep(KEPT_UNUSED / 10);
            !fixture.search.built()
        });
    }

    #[test]
    fn a_launcher_opened_again_soon_finds_it_still_built() {
        let fixture = Fixture::new("reopened", steam_game);
        fixture.search.set_used(FILES, Use::Files, false);
        std::thread::sleep(KEPT_UNUSED / 2);
        fixture.search.set_used(UI, Use::Launcher, true);
        std::thread::sleep(KEPT_UNUSED * 2);
        assert!(fixture.search.built(), "never dropped");
        assert_eq!(fixture.find("crimson").total, 2);
    }

    #[test]
    fn a_search_with_nobody_using_the_index_builds_it_for_a_while() {
        let fixture = Fixture::unused("unused-search", steam_game);
        wait_until(WAIT, "a search finds it", || fixture.find("crimson").total == 2);
        fixture.wait_built(false);
        wait_until(WAIT, "the next search builds it again", || fixture.find("crimson").total == 2);
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
