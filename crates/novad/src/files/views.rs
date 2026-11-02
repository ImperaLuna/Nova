//! Folder views (`files_open`, `files_close`): each client's open folders.
//!
//! Opening a view checks the folder can be read, then a thread of its own
//! reads it, puts it in the view's order and sends it in pages of
//! [`PAGE_ENTRIES`], so a big folder never holds up the connection.
//!
//! Every page is sent while holding the view's state, after checking the
//! view is still open, so nothing arrives after `files_close` or after
//! another `files_open` replaced it.

use crate::files::folder;
use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::mime::MimeDb;
use nova_protocol::{Event, FILES_MAX_VIEWS, FILES_PATH_MAX_BYTES, FileSort, FolderPage};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

const LOG_SOURCE: &str = "files";
/// Entries per `files_page` event: a few hundred KB of JSON at most.
pub const PAGE_ENTRIES: usize = 500;

/// A view, named by its client and the client's number for it.
type Key = (ClientId, u32);

/// The open folder views.
pub struct Folders {
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    mime: Arc<MimeDb>,
    views: Mutex<HashMap<Key, Arc<View>>>,
}

struct View {
    key: Key,
    path: PathBuf,
    /// The path as `files_open` named it, for the pages.
    named: String,
    sort: FileSort,
    hidden: bool,
    /// Still open: pages are sent only while it is.
    open: Mutex<bool>,
}

impl Folders {
    pub fn new(hub: Arc<Hub>, logger: Arc<Logger>, mime: Arc<MimeDb>) -> Self {
        Self { hub, logger, mime, views: Mutex::default() }
    }

    /// Opens `path` in `client`'s view `view`, replacing what it showed.
    /// Returns once the folder can be read; its entries follow as events.
    pub fn open(&self, client: ClientId, view: u32, path: &str, sort: FileSort, hidden: bool) -> Result<(), String> {
        let folder = checked_path(path)?;
        let key = (client, view);
        // Off the views' lock: a slow disk delays only this view.
        std::fs::read_dir(&folder).map_err(|e| unreadable(path, &e))?;
        let mut views = lock(&self.views);
        if !views.contains_key(&key) && views.keys().filter(|(owner, _)| *owner == client).count() >= FILES_MAX_VIEWS {
            return Err(format!("At most {FILES_MAX_VIEWS} folders can be open at once"));
        }
        if let Some(old) = views.remove(&key) {
            old.close();
        }
        let opened = Arc::new(View { key, path: folder, named: path.into(), sort, hidden, open: Mutex::new(true) });
        views.insert(key, Arc::clone(&opened));
        drop(views);
        let worker = Worker {
            view: opened,
            hub: Arc::clone(&self.hub),
            logger: Arc::clone(&self.logger),
            mime: Arc::clone(&self.mime),
        };
        std::thread::Builder::new()
            .name("novad-files-view".into())
            .spawn(move || worker.send_all())
            .map(drop)
            .map_err(|e| format!("Can't read the folder: {e}"))
    }

    /// Closes `client`'s view `view`; nothing more is sent for it.
    pub fn close(&self, client: ClientId, view: u32) {
        if let Some(old) = lock(&self.views).remove(&(client, view)) {
            old.close();
        }
    }

    /// Closes every view of `client`, which hung up.
    pub fn client_gone(&self, client: ClientId) {
        lock(&self.views).retain(|(owner, _), view| {
            let keep = *owner != client;
            if !keep {
                view.close();
            }
            keep
        });
    }

    /// Closes every view.
    pub fn stop(&self) {
        for (_, view) in lock(&self.views).drain() {
            view.close();
        }
    }
}

impl View {
    fn close(&self) {
        *lock(&self.open) = false;
    }
}

/// The thread that reads one view's folder.
struct Worker {
    view: Arc<View>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    mime: Arc<MimeDb>,
}

impl Worker {
    /// Reads the whole folder and sends it from the start.
    fn send_all(&self) {
        let mut entries = match folder::read_folder(&self.view.path, self.view.hidden, &self.mime) {
            Ok(entries) => entries,
            Err(e) => {
                return self.logger.warn(LOG_SOURCE, &format!("can't read {}: {e}", self.view.path.display()));
            }
        };
        entries.sort_by(|a, b| folder::order(self.view.sort, a, b));
        let open = lock(&self.view.open);
        if !*open {
            return;
        }
        let (client, view) = self.view.key;
        let total = u32::try_from(entries.len()).unwrap_or(u32::MAX);
        // An empty folder still gets its one page.
        let pages = entries.chunks(PAGE_ENTRIES).map(<[_]>::to_vec).chain(entries.is_empty().then(Vec::new));
        for (number, entries) in pages.enumerate() {
            let start = u32::try_from(number * PAGE_ENTRIES).unwrap_or(u32::MAX);
            let page = FolderPage { view, path: self.view.named.clone(), start, total, entries };
            self.hub.send_to(client, &Event::FilesPage(page));
        }
    }
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
    // slashes are dropped so the pages name one folder.
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
    use nova_protocol::{FileEntry, FileSortKey, LogLevel};
    use nova_test_support::TempDir;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

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
            let (mime, problems) = MimeDb::load(&[crate::mime::tests::data_dir(&format!("{name}-mime")).to_path_buf()]);
            assert!(problems.is_empty(), "{problems:?}");
            let (socket, ours) = UnixStream::pair().unwrap();
            let (queue, events) = ClientQueue::new(&ours).unwrap();
            let client = hub.register(queue);
            let folders = Folders::new(hub, logger, Arc::new(mime));
            Self { logs, folders, client, events, _socket: socket, shown: Vec::new() }
        }

        fn open(&self, folder: &Path) -> Result<(), String> {
            self.folders.open(self.client, 1, folder.to_str().unwrap(), BY_NAME, false)
        }

        /// Takes the next event and applies it to `shown` as the UI would.
        fn next(&mut self) -> serde_json::Value {
            let event = next_event(&self.events, WAIT);
            if event["event"] == "files_page" {
                let page: FolderPage = serde_json::from_value(event["data"].clone()).unwrap();
                if page.start == 0 {
                    self.shown.clear();
                }
                assert_eq!(page.start as usize, self.shown.len(), "pages come in order");
                assert!(page.entries.len() <= PAGE_ENTRIES);
                self.shown.extend(page.entries);
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

        /// No more events come.
        fn nothing_more(&self) {
            std::thread::sleep(Duration::from_millis(500));
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
    fn opening_again_replaces_the_view() {
        let mut fixture = Fixture::new("files-replace");
        let (first, second) = (TempDir::new("files-replace-a"), TempDir::new("files-replace-b"));
        std::fs::write(first.join("a.md"), b"").unwrap();
        std::fs::write(second.join("b.md"), b"").unwrap();
        fixture.open(&first).unwrap();
        fixture.open(&second).unwrap();
        fixture.until_shown(&["b.md"]);
        fixture.nothing_more();
    }

    #[test]
    fn only_readable_absolute_folders_open() {
        let fixture = Fixture::new("files-refused");
        let dir = TempDir::new("files-refused-folder");
        std::fs::write(dir.join("note.md"), b"").unwrap();
        let open = |path: &str| fixture.folders.open(fixture.client, 1, path, BY_NAME, false);
        assert!(open("relative/folder").unwrap_err().contains("absolute"));
        assert!(open(dir.join("missing").to_str().unwrap()).unwrap_err().contains("doesn't exist"));
        assert!(open(dir.join("note.md").to_str().unwrap()).unwrap_err().contains("isn't a folder"));
        assert!(open(&format!("/{}", "x".repeat(FILES_PATH_MAX_BYTES))).is_err());
        let views = (0..=FILES_MAX_VIEWS as u32)
            .map(|view| fixture.folders.open(fixture.client, view, dir.to_str().unwrap(), BY_NAME, false));
        assert_eq!(views.filter(Result::is_err).count(), 1, "one view past the limit");
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
