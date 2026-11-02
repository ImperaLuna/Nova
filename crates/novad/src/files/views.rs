//! Folder views (`files_open`, `files_close`): each client's open folders,
//! kept up to date while they're open.
//!
//! Opening a view only records it, so neither a big folder nor a stalled
//! mount holds up the connection. Each view has its own thread. It watches
//! the folder with inotify first and reads it after, so nothing written in
//! between is missed, then sends
//! the entries in pages of [`PAGE_ENTRIES`]. From then on it gathers the
//! names inotify reports until the folder has been quiet for [`QUIET`]
//! (or [`MOST_DELAY`] has passed, so a long copy still shows), reads those
//! entries again and sends what changed as index operations
//! ([`super::folder::Listing::apply`]). A full inotify queue means lost
//! events, so the folder is read and sent again from the start. The
//! folder's own entry is watched in its parent too: when it's replaced (a
//! symlink pointed elsewhere, another folder renamed over it) the view
//! follows the path to what it names now. When the folder can't be read
//! or goes, the view closes with `files_closed`.
//! Reading stops as soon as the view closes.
//!
//! Every event of a view is sent while holding its state, after checking
//! the view is still open, so nothing arrives after `files_close` or after
//! another `files_open` replaced it.

use crate::files::folder::{self, Listing};
use crate::hub::{ClientId, Hub};
use crate::inotify::{self, Inotify, Stopper};
use crate::lock;
use crate::logger::Logger;
use crate::mime::Mime;
use nova_protocol::{Event, FILES_MAX_VIEWS, FILES_PATH_MAX_BYTES, FileSort, FolderPage};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG_SOURCE: &str = "files";
/// Entries per `files_page` event: a few hundred KB of JSON at most.
pub const PAGE_ENTRIES: usize = 500;
/// How long a folder stays quiet before its changes are sent.
const QUIET: Duration = Duration::from_millis(50);
/// The longest changes wait while a folder keeps changing.
const MOST_DELAY: Duration = Duration::from_millis(250);
/// What the watch reports: entries coming, going, written or touched, and
/// the folder itself going.
const WATCHED: u32 = inotify::ENTRIES
    | libc::IN_MODIFY
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | inotify::ITSELF
    | libc::IN_ONLYDIR
    | libc::IN_EXCL_UNLINK;

/// A view, named by its client and the client's number for it.
type Key = (ClientId, u32);

/// The open views; `None` once the module stopped, so a `files_open`
/// already on its way can't start a view after that.
type Views = Arc<Mutex<Option<HashMap<Key, Arc<View>>>>>;

/// The open folder views.
pub struct Folders {
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    mime: Arc<Mime>,
    views: Views,
}

struct View {
    key: Key,
    path: PathBuf,
    /// The path as `files_open` named it, for the pages.
    named: String,
    sort: FileSort,
    hidden: bool,
    state: Mutex<State>,
    stopper: Stopper,
}

struct State {
    open: bool,
    listing: Option<Listing>,
}

impl Folders {
    pub fn new(hub: Arc<Hub>, logger: Arc<Logger>, mime: Arc<Mime>) -> Self {
        Self { hub, logger, mime, views: Arc::new(Mutex::new(Some(HashMap::new()))) }
    }

    /// Opens `path` in `client`'s view `view`, replacing what it showed.
    /// Doesn't touch the disk: the entries, or why they can't be read,
    /// follow as events.
    pub fn open(&self, client: ClientId, view: u32, path: &str, sort: FileSort, hidden: bool) -> Result<(), String> {
        let folder = checked_path(path)?;
        let key = (client, view);
        let watcher = Inotify::new(WATCHED).map_err(|e| format!("Can't watch folders: {e}"))?;
        let mut views = lock(&self.views);
        let Some(open) = views.as_mut() else { return Err("Nova Files is off".into()) };
        if !open.contains_key(&key) && open.keys().filter(|(owner, _)| *owner == client).count() >= FILES_MAX_VIEWS {
            return Err(format!("At most {FILES_MAX_VIEWS} folders can be open at once"));
        }
        if let Some(old) = open.remove(&key) {
            old.close();
        }
        let opened = Arc::new(View {
            key,
            path: folder,
            named: path.into(),
            sort,
            hidden,
            state: Mutex::new(State { open: true, listing: None }),
            stopper: watcher.stopper(),
        });
        open.insert(key, Arc::clone(&opened));
        drop(views);
        let worker = Worker {
            view: opened,
            watcher,
            hub: Arc::clone(&self.hub),
            logger: Arc::clone(&self.logger),
            mime: Arc::clone(&self.mime),
            views: Arc::clone(&self.views),
        };
        let spawned = std::thread::Builder::new().name("novad-files-view".into()).spawn(move || worker.run());
        spawned.map(drop).map_err(|e| {
            self.close(client, view);
            format!("Can't follow the folder: {e}")
        })
    }

    /// Closes `client`'s view `view`; nothing more is sent for it.
    pub fn close(&self, client: ClientId, view: u32) {
        if let Some(old) = lock(&self.views).as_mut().and_then(|open| open.remove(&(client, view))) {
            old.close();
        }
    }

    /// Closes every view of `client`, which hung up.
    pub fn client_gone(&self, client: ClientId) {
        let mut views = lock(&self.views);
        let Some(open) = views.as_mut() else { return };
        open.retain(|(owner, _), view| {
            let keep = *owner != client;
            if !keep {
                view.close();
            }
            keep
        });
    }

    /// Closes every view, for good.
    pub fn stop(&self) {
        for (_, view) in lock(&self.views).take().into_iter().flatten() {
            view.close();
        }
    }
}

impl View {
    fn close(&self) {
        lock(&self.state).open = false;
        self.stopper.stop();
    }

    fn is_open(&self) -> bool {
        lock(&self.state).open
    }
}

/// The thread that reads and follows one view's folder.
struct Worker {
    view: Arc<View>,
    watcher: Inotify,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    mime: Arc<Mime>,
    views: Views,
}

impl Worker {
    fn run(mut self) {
        // Best effort: a parent that can't be listed still lets the folder
        // be followed, only not replaced.
        if let Some(parent) = self.view.path.parent() {
            let _ = self.watcher.watch_for(parent, inotify::ENTRIES);
        }
        if let Err(e) = self.watcher.watch(&self.view.path) {
            return self.closed(unreadable(&self.view.named, &e));
        }
        if !self.send_all() {
            return;
        }
        loop {
            match self.gather() {
                Ok(Some(Gathered::Names(names))) => {
                    if !self.send_changes(names) {
                        return;
                    }
                }
                // Lost events may include the folder being replaced.
                Ok(Some(Gathered::Lost | Gathered::Moved)) => {
                    if !self.reattach() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    self.logger.warn(LOG_SOURCE, &format!("stopped following {}: {e}", self.view.path.display()));
                    return self.closed(format!("Stopped following {}: {e}", self.view.named));
                }
            }
        }
    }

    /// Watches what the path names now and sends it from the start, or
    /// closes the view when nothing is there. False once the view is closed.
    fn reattach(&mut self) -> bool {
        self.watcher.unwatch(&self.view.path);
        match self.watcher.watch(&self.view.path) {
            Ok(()) => self.send_all(),
            Err(e) => {
                let error = match e.kind() {
                    std::io::ErrorKind::NotFound => format!("{} was deleted or moved", self.view.named),
                    _ => unreadable(&self.view.named, &e),
                };
                self.closed(error);
                false
            }
        }
    }

    /// Reads the whole folder and sends it from the start. False once the
    /// view is closed.
    fn send_all(&self) -> bool {
        let read =
            folder::read_folder(&self.view.path, self.view.hidden, &self.mime.current(), || !self.view.is_open());
        let entries = match read {
            Ok(Some(entries)) => entries,
            Ok(None) => return false,
            Err(e) => {
                self.logger.warn(LOG_SOURCE, &format!("can't read {}: {e}", self.view.path.display()));
                self.closed(unreadable(&self.view.named, &e));
                return false;
            }
        };
        let listing = Listing::new(self.view.sort, self.view.hidden, entries);
        let mut state = lock(&self.view.state);
        if !state.open {
            return false;
        }
        let (client, view) = self.view.key;
        let all = listing.entries();
        let total = u32::try_from(all.len()).unwrap_or(u32::MAX);
        // An empty folder still gets its one page.
        let pages = all.chunks(PAGE_ENTRIES).map(<[_]>::to_vec).chain(all.is_empty().then(Vec::new));
        for (number, entries) in pages.enumerate() {
            let start = u32::try_from(number * PAGE_ENTRIES).unwrap_or(u32::MAX);
            let page = FolderPage { view, path: self.view.named.clone(), start, total, entries };
            self.hub.send_to(client, &Event::FilesPage(page));
        }
        state.listing = Some(listing);
        true
    }

    /// Waits for changes and gathers them until the folder is quiet.
    /// `None` once the view is closed.
    fn gather(&mut self) -> std::io::Result<Option<Gathered>> {
        let Some(first) = self.watcher.read()? else { return Ok(None) };
        let mut names = BTreeSet::new();
        let mut changes = first;
        let most = Instant::now() + MOST_DELAY;
        loop {
            // The folder went, or its entry in the parent changed.
            if !changes.gone.is_empty() || changes.found.iter().any(|change| change.path == self.view.path) {
                return Ok(Some(Gathered::Moved));
            }
            if changes.overflowed {
                return Ok(Some(Gathered::Lost));
            }
            // The parent's other entries don't matter.
            for change in changes.found {
                if change.path.parent() == Some(&self.view.path)
                    && let Some(name) = change.path.file_name()
                {
                    names.insert(name.to_owned());
                }
            }
            // A folder that never goes quiet still shows its changes.
            if Instant::now() >= most {
                return Ok(Some(Gathered::Names(names)));
            }
            let deadline = (Instant::now() + QUIET).min(most);
            let Some(next) = self.watcher.read_until(Some(deadline))? else { return Ok(None) };
            // Empty at the deadline, or only events about the folder itself.
            if next.is_empty() && Instant::now() >= deadline {
                return Ok(Some(Gathered::Names(names)));
            }
            changes = next;
        }
    }

    /// Reads the entries `names` again and sends what changed. False once
    /// the view is closed.
    fn send_changes(&self, names: BTreeSet<OsString>) -> bool {
        let mime = self.mime.current();
        let now: Vec<_> = names
            .into_iter()
            .map(|name| (folder::read_entry(&self.view.path, &name, &mime), name.to_string_lossy().into_owned()))
            .collect();
        let mut state = lock(&self.view.state);
        if !state.open {
            return false;
        }
        let Some(listing) = state.listing.as_mut() else { return true };
        let changes: Vec<_> = now.into_iter().flat_map(|(entry, name)| listing.apply(&name, entry)).collect();
        if !changes.is_empty() {
            let (client, view) = self.view.key;
            self.hub.send_to(client, &Event::FilesChanged { view, changes });
        }
        true
    }

    /// Closes the view for `error`, telling its client once.
    fn closed(&self, error: String) {
        let mut views = lock(&self.views);
        let mut state = lock(&self.view.state);
        if !state.open {
            return;
        }
        state.open = false;
        if let Some(views) = views.as_mut()
            && views.get(&self.view.key).is_some_and(|shown| Arc::ptr_eq(shown, &self.view))
        {
            views.remove(&self.view.key);
        }
        let (client, view) = self.view.key;
        self.hub.send_to(client, &Event::FilesClosed { view, error });
    }
}

/// What a wait for changes found.
enum Gathered {
    /// Entries that may have changed.
    Names(BTreeSet<OsString>),
    /// Events were lost: read everything again.
    Lost,
    /// The folder itself went or was replaced.
    Moved,
}

/// `path` from the socket, if it's a usable absolute path.
fn checked_path(path: &str) -> Result<PathBuf, String> {
    if path.len() > FILES_PATH_MAX_BYTES || path.contains('\0') {
        return Err("Not a path".into());
    }
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("The path must be absolute".into());
    }
    // `..` is resolved by the kernel like any path; `.` and repeated
    // slashes are dropped so the watch and the pages name one folder.
    Ok(path.components().filter(|part| *part != Component::CurDir).collect())
}

fn unreadable(path: &str, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => format!("{path} doesn't exist"),
        std::io::ErrorKind::PermissionDenied => format!("No permission to open {path}"),
        std::io::ErrorKind::NotADirectory => format!("{path} isn't a folder"),
        _ => format!("Can't open {path}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing, next_event};
    use nova_protocol::{FileEntry, FileSortKey, FolderChange, LogLevel};
    use nova_test_support::TempDir;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(5);
    const BY_NAME: FileSort = FileSort { key: FileSortKey::Name, descending: false };

    /// The views of one client, and what the UI would hold for its view 1.
    struct Fixture {
        logs: TempDir,
        folders: Folders,
        client: ClientId,
        events: Receiver<Outgoing>,
        _socket: UnixStream,
        shown: Vec<FileEntry>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let logs = TempDir::new(&format!("{name}-logs"));
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&logs.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let mime = Mime::follow(
                vec![crate::mime::tests::data_dir(&format!("{name}-mime")).to_path_buf()],
                Arc::clone(&logger),
            );
            let (socket, ours) = UnixStream::pair().unwrap();
            let (queue, events) = ClientQueue::new(&ours).unwrap();
            let client = hub.register(queue);
            let folders = Folders::new(hub, logger, mime);
            Self { logs, folders, client, events, _socket: socket, shown: Vec::new() }
        }

        fn open(&self, folder: &Path) -> Result<(), String> {
            self.folders.open(self.client, 1, folder.to_str().unwrap(), BY_NAME, false)
        }

        /// Takes the next event and applies it to `shown` as the UI would.
        fn next(&mut self) -> serde_json::Value {
            let event = next_event(&self.events, WAIT);
            match event["event"].as_str() {
                Some("files_page") => {
                    let page: FolderPage = serde_json::from_value(event["data"].clone()).unwrap();
                    if page.start == 0 {
                        self.shown.clear();
                    }
                    assert_eq!(page.start as usize, self.shown.len(), "pages come in order");
                    assert!(page.entries.len() <= PAGE_ENTRIES);
                    self.shown.extend(page.entries);
                }
                Some("files_changed") => {
                    let changes: Vec<FolderChange> = serde_json::from_value(event["data"]["changes"].clone()).unwrap();
                    for change in changes {
                        match change {
                            FolderChange::Insert { index, entry } => self.shown.insert(index as usize, entry),
                            FolderChange::Remove { index } => drop(self.shown.remove(index as usize)),
                            FolderChange::Update { index, entry } => self.shown[index as usize] = entry,
                        }
                    }
                }
                _ => {}
            }
            event
        }

        /// Applies events until the view shows `names`, in that order.
        fn until_shown(&mut self, names: &[&str]) {
            while self.names() != names {
                self.next();
            }
        }

        fn names(&self) -> Vec<&str> {
            self.shown.iter().map(|entry| entry.name.as_str()).collect()
        }

        /// No event comes within the quiet time and the delay after it.
        fn nothing_more(&self) {
            std::thread::sleep(MOST_DELAY * 2);
            let pending: Vec<_> = std::iter::from_fn(|| match self.events.try_recv() {
                Ok(Outgoing::Line(line)) => Some(line),
                _ => None,
            })
            .collect();
            assert!(pending.is_empty(), "unexpected events: {pending:?}");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.folders.stop();
            let _ = &self.logs;
        }
    }

    #[test]
    fn a_folder_comes_in_pages_in_its_order() {
        let mut fixture = Fixture::new("files-pages");
        let dir = TempDir::new("files-pages-folder");
        let files = PAGE_ENTRIES * 2 + 2;
        for number in 0..files {
            std::fs::write(dir.join(format!("photo {number}.png")), b"x").unwrap();
        }
        std::fs::create_dir(dir.join("Zeta")).unwrap();
        std::fs::create_dir(dir.join("alpha")).unwrap();
        std::fs::write(dir.join(".hidden"), b"").unwrap();
        fixture.open(&dir).unwrap();
        let first = fixture.next();
        assert_eq!(first["data"]["total"], files + 2);
        assert_eq!(first["data"]["path"], dir.to_str().unwrap());
        while fixture.shown.len() < files + 2 {
            fixture.next();
        }
        let names = fixture.names();
        assert_eq!(names[..4], ["alpha", "Zeta", "photo 0.png", "photo 1.png"]);
        assert_eq!(names.last(), Some(&format!("photo {}.png", files - 1).as_str()));
        let photo = &fixture.shown[2];
        assert_eq!((photo.mime.as_str(), photo.size, photo.dir), ("image/png", Some(1), false));
        assert_eq!(fixture.shown[0].mime, "inode/directory");
        fixture.nothing_more();
    }

    #[test]
    fn an_empty_folder_gets_one_empty_page() {
        let mut fixture = Fixture::new("files-empty");
        let dir = TempDir::new("files-empty-folder");
        fixture.open(&dir).unwrap();
        let page = fixture.next();
        assert_eq!(page["event"], "files_page");
        assert_eq!(page["data"]["total"], 0);
        assert_eq!(page["data"]["entries"], serde_json::json!([]));
    }

    /// Made, renamed, written and deleted from outside, as a terminal would.
    #[test]
    fn changes_from_other_programs_show_by_themselves() {
        let mut fixture = Fixture::new("files-live");
        let dir = TempDir::new("files-live-folder");
        std::fs::write(dir.join("b.md"), b"").unwrap();
        fixture.open(&dir).unwrap();
        fixture.until_shown(&["b.md"]);
        std::fs::write(dir.join("a.md"), b"").unwrap();
        std::fs::create_dir(dir.join("Notes")).unwrap();
        fixture.until_shown(&["Notes", "a.md", "b.md"]);
        std::fs::rename(dir.join("b.md"), dir.join("0.md")).unwrap();
        fixture.until_shown(&["Notes", "0.md", "a.md"]);
        std::fs::write(dir.join("a.md"), b"# notes").unwrap();
        while fixture.shown[2].size != Some(7) {
            fixture.next();
        }
        std::fs::remove_dir(dir.join("Notes")).unwrap();
        std::fs::write(dir.join(".secret"), b"").unwrap();
        fixture.until_shown(&["0.md", "a.md"]);
        fixture.nothing_more();
    }

    /// A file written to without pause, like one being copied, still shows
    /// its changes while it grows.
    #[test]
    fn a_folder_that_never_goes_quiet_still_shows_changes() {
        let mut fixture = Fixture::new("files-busy");
        let dir = TempDir::new("files-busy-folder");
        fixture.open(&dir).unwrap();
        fixture.next();
        let path = dir.join("copying.bin");
        let writing = MOST_DELAY * 6;
        let writer = std::thread::spawn(move || {
            let started = Instant::now();
            let mut file = std::fs::File::create(path).unwrap();
            while started.elapsed() < writing {
                std::io::Write::write_all(&mut file, b"x").unwrap();
                std::thread::sleep(QUIET / 3);
            }
        });
        let started = Instant::now();
        while fixture.next()["event"] != "files_changed" {}
        assert!(started.elapsed() < writing - MOST_DELAY, "changes waited for the writes to stop");
        writer.join().unwrap();
    }

    /// Names from old archives or Windows drives needn't be UTF-8.
    #[test]
    fn a_name_that_isnt_utf8_is_listed() {
        use std::os::unix::ffi::OsStrExt;
        let mut fixture = Fixture::new("files-latin1");
        let dir = TempDir::new("files-latin1-folder");
        std::fs::write(dir.join(std::ffi::OsStr::from_bytes(b"caf\xe9.md")), b"").unwrap();
        fixture.open(&dir).unwrap();
        fixture.until_shown(&["caf\u{fffd}.md"]);
    }

    #[test]
    fn a_deleted_folder_closes_its_view() {
        let mut fixture = Fixture::new("files-gone");
        let dir = TempDir::new("files-gone-folder");
        let folder = dir.join("going");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("a.md"), b"").unwrap();
        fixture.open(&folder).unwrap();
        fixture.until_shown(&["a.md"]);
        std::fs::remove_dir_all(&folder).unwrap();
        while fixture.next()["event"] != "files_closed" {}
        assert!(lock(&fixture.folders.views).as_ref().unwrap().is_empty());
        fixture.nothing_more();
    }

    /// A symlink pointed elsewhere shows, and follows, its new folder.
    #[test]
    fn a_replaced_symlink_is_followed() {
        let mut fixture = Fixture::new("files-relink");
        let dir = TempDir::new("files-relink-folder");
        let (old, new, link) = (dir.join("old"), dir.join("new"), dir.join("link"));
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("a.md"), b"").unwrap();
        std::fs::write(new.join("b.md"), b"").unwrap();
        std::os::unix::fs::symlink(&old, &link).unwrap();
        fixture.open(&link).unwrap();
        fixture.until_shown(&["a.md"]);
        std::os::unix::fs::symlink(&new, dir.join("link.new")).unwrap();
        std::fs::rename(dir.join("link.new"), &link).unwrap();
        fixture.until_shown(&["b.md"]);
        std::fs::write(new.join("c.md"), b"").unwrap();
        fixture.until_shown(&["b.md", "c.md"]);
        std::fs::write(old.join("x.md"), b"").unwrap();
        std::fs::write(dir.join("beside.md"), b"").unwrap();
        fixture.nothing_more();
    }

    #[test]
    fn nothing_comes_after_a_view_closes() {
        let mut fixture = Fixture::new("files-closed");
        let dir = TempDir::new("files-closed-folder");
        fixture.open(&dir).unwrap();
        fixture.next();
        fixture.folders.close(fixture.client, 1);
        std::fs::write(dir.join("late.md"), b"").unwrap();
        fixture.nothing_more();
    }

    #[test]
    fn opening_again_replaces_the_view() {
        let mut fixture = Fixture::new("files-replace");
        let (first, second) = (TempDir::new("files-replace-a"), TempDir::new("files-replace-b"));
        std::fs::write(first.join("a.md"), b"").unwrap();
        std::fs::write(second.join("b.md"), b"").unwrap();
        fixture.open(&first).unwrap();
        fixture.open(&second).unwrap();
        fixture.until_shown(&["b.md"]);
        std::fs::write(first.join("late.md"), b"").unwrap();
        fixture.nothing_more();
    }

    #[test]
    fn only_readable_absolute_folders_open() {
        let fixture = Fixture::new("files-refused");
        let dir = TempDir::new("files-refused-folder");
        std::fs::write(dir.join("note.md"), b"").unwrap();
        let open = |path: &str| fixture.folders.open(fixture.client, 1, path, BY_NAME, false);
        assert!(open("relative/folder").unwrap_err().contains("absolute"));
        assert!(open(&format!("/{}", "x".repeat(FILES_PATH_MAX_BYTES))).is_err());
        let views = (0..=FILES_MAX_VIEWS as u32)
            .map(|view| fixture.folders.open(fixture.client, view, dir.to_str().unwrap(), BY_NAME, false));
        assert_eq!(views.filter(Result::is_err).count(), 1, "one view past the limit");
    }

    #[test]
    fn a_folder_that_cant_be_read_closes_its_view() {
        let mut fixture = Fixture::new("files-unreadable");
        let dir = TempDir::new("files-unreadable-folder");
        std::fs::write(dir.join("note.md"), b"").unwrap();
        for (path, why) in [(dir.join("missing"), "doesn't exist"), (dir.join("note.md"), "isn't a folder")] {
            fixture.open(&path).unwrap();
            let closed = fixture.next();
            assert_eq!(closed["event"], "files_closed");
            assert_eq!(closed["data"]["view"], 1);
            assert!(closed["data"]["error"].as_str().unwrap().contains(why), "{closed}");
        }
        // The closed view no longer counts, and opens again.
        fixture.open(&dir).unwrap();
        fixture.until_shown(&["note.md"]);
    }

    #[test]
    fn nothing_opens_after_stop() {
        let fixture = Fixture::new("files-stopped");
        let dir = TempDir::new("files-stopped-folder");
        fixture.folders.stop();
        assert!(fixture.open(&dir).is_err());
        fixture.nothing_more();
    }

    #[test]
    fn closing_stops_reading_the_folder() {
        let fixture = Fixture::new("files-cancel");
        let view = Arc::new(View {
            key: (fixture.client, 1),
            path: PathBuf::from("/"),
            named: "/".into(),
            sort: BY_NAME,
            hidden: false,
            state: Mutex::new(State { open: false, listing: None }),
            stopper: Inotify::new(WATCHED).unwrap().stopper(),
        });
        let read = folder::read_folder(&view.path, true, &fixture.folders.mime.current(), || !view.is_open());
        assert!(read.unwrap().is_none());
    }

    /// The ticket's bar: 100,000 files open without a stall.
    #[test]
    fn a_hundred_thousand_files_open() {
        let mut fixture = Fixture::new("files-big");
        let dir = TempDir::new("files-big-folder");
        let files = 100_000;
        for number in 0..files {
            std::fs::File::create(dir.join(format!("{number}.png"))).unwrap();
        }
        let started = Instant::now();
        fixture.open(&dir).unwrap();
        while fixture.shown.len() < files {
            fixture.next();
        }
        eprintln!("{files} files listed in {:?}", started.elapsed());
        assert_eq!(fixture.shown[files - 1].name, format!("{}.png", files - 1));
    }
}
