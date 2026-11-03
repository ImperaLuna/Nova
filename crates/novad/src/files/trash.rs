//! The trash, shared with every other app: the freedesktop Trash spec 1.0
//! (<https://specifications.freedesktop.org/trash-spec/1.0/>).
//!
//! An item goes to the trash on its own drive, so trashing is one rename
//! and never a copy: the home trash (`$XDG_DATA_HOME/Trash`) for items on
//! the same drive as it, else the drive's own, at the top of the drive
//! (`$top/.Trash/$uid` when the admin made `$top/.Trash` a sticky folder,
//! else `$top/.Trash-$uid`, made when first needed). Each trashed item is
//! `files/<name>` plus `info/<name>.trashinfo`, which holds where it came
//! from (percent-escaped; relative to the drive's top in a drive trash)
//! and when, in local time. The info file is made first, with create-new:
//! that is how two apps trashing at once each get their own name.
//!
//! Over the socket an item is named by its id: its info file's path,
//! percent-escaped, so names that aren't UTF-8 still round-trip; names
//! and folders go as wire text (`nova_protocol::wire_text`). Every id is
//! checked to be an info file in a trash that exists now before anything
//! is touched, and every path to trash or delete must be absolute with no
//! `..`, so nothing from the socket reaches outside what it names.
//!
//! The list is read anew each time it's asked for (`files_trash_list`).
//! [`Watch`] follows every trash's `info/` with inotify and publishes
//! `files_trash_changed` when anything, in any app, trashes, restores or
//! empties, so the windows ask again. Drives come and go, so it looks for
//! trashes again after each change and every [`RESCAN`]: a stat or two
//! per local drive.

use crate::files::folder;
use crate::hub::Hub;
use crate::inotify::{self, Inotify, Stopper};
use crate::logger::Logger;
use crate::mime::Mime;
use crate::notifications::image::percent_decoded;
use nova_protocol::{Event, FILES_PATH_MAX_WIRE_BYTES, TrashItem, wire_text};
use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const LOG_SOURCE: &str = "files";
/// The folder of a trash holding the items.
const FILES: &str = "files";
/// The folder of a trash holding where each item came from.
const INFO: &str = "info";
/// An info file's name is its item's plus this.
const INFO_SUFFIX: &str = ".trashinfo";
/// The cache of trashed folders' sizes some apps keep; emptying clears it.
const DIRECTORY_SIZES: &str = "directorysizes";
/// The one group an info file has.
const INFO_GROUP: &str = "[Trash Info]";
const PATH_KEY: &str = "Path=";
const DATE_KEY: &str = "DeletionDate=";
/// `DeletionDate`'s form: local time, no zone.
const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
/// A drive's shared trash folder, which holds one folder per user.
const SHARED_TRASH: &str = ".Trash";
/// A drive's per-user trash folder is this plus the user's id.
const USER_TRASH_PREFIX: &str = ".Trash-";
/// Trash folders are the user's alone.
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
/// Names tried for one item before giving up: `name`, `name.2`, and on.
const MOST_NAME_TRIES: u32 = 10_000;
/// The longest name a folder entry takes on Linux's filesystems.
const NAME_MAX_BYTES: usize = 255;
/// Room for the `.N` a taken name gets, up to `.10000`.
const NAME_TRY_ROOM: usize = 6;
/// Bytes kept as they are in an info file's path; the rest are escaped.
const PATH_SAFE: &[u8] = b"-_.~/";
/// Drives whose trash is looked for: local disks and sticks. Network and
/// in-memory filesystems are left out, so a stalled share never holds up
/// the list.
const DISK_FILESYSTEMS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "bcachefs", "zfs", "jfs", "reiserfs", "vfat", "exfat", "ntfs",
    "ntfs3", "fuseblk", "hfsplus",
];
/// How often trashes are looked for again, for drives that came or went.
const RESCAN: Duration = Duration::from_secs(10);
/// How long the trash stays quiet before a change is published: emptying
/// a big trash is one change, not thousands.
const QUIET: Duration = Duration::from_millis(100);

/// The trashes this user has: the home one and one per drive.
pub struct Trash {
    /// `$XDG_DATA_HOME/Trash`; none without a home.
    home: Option<PathBuf>,
    /// The mount table (`/proc/self/mountinfo`).
    mounts: PathBuf,
    uid: u32,
    mime: Arc<Mime>,
}

/// One trash: `files/` and `info/` under `dir`.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Root {
    dir: PathBuf,
    /// The drive's top folder, which the paths in a drive trash's info
    /// files may be relative to; none for the home trash.
    top: Option<PathBuf>,
}

impl Root {
    fn files(&self) -> PathBuf {
        self.dir.join(FILES)
    }

    fn info(&self) -> PathBuf {
        self.dir.join(INFO)
    }
}

/// An item in a trash, found by its id.
struct Found {
    root: Root,
    /// Its name in `files/`.
    name: OsString,
    info: PathBuf,
}

impl Found {
    fn file(&self) -> PathBuf {
        self.root.files().join(&self.name)
    }
}

impl Trash {
    pub fn new(home: Option<PathBuf>, mounts: PathBuf, mime: Arc<Mime>) -> Self {
        // SAFETY: no arguments; it can't fail.
        let uid = unsafe { libc::getuid() };
        Self { home, mounts, uid, mime }
    }

    /// Moves each of `paths` to its drive's trash: each one's id there, or
    /// why it stayed.
    pub fn trash_each(&self, paths: &[String]) -> Vec<Result<String, String>> {
        paths.iter().map(|path| self.trash_one(&checked(path)?)).collect()
    }

    /// Deletes each of `paths` for good, folders with all they hold.
    pub fn delete(paths: &[String]) -> Result<(), String> {
        each(paths, |path| remove(&checked(path)?).map_err(|e| failed("delete", Path::new(path), &e)))
    }

    /// What every trash holds, the last trashed first.
    pub fn list(&self) -> Vec<TrashItem> {
        let mut items: Vec<TrashItem> = self.roots().iter().flat_map(|root| self.items(root)).collect();
        items.sort_by(|a, b| b.deleted_ms.cmp(&a.deleted_ms).then_with(|| a.entry.name.cmp(&b.entry.name)));
        items
    }

    /// Puts each item `ids` names back where it came from, making its
    /// folder again if it's gone. An item whose place is taken stays.
    pub fn restore(&self, ids: &[String]) -> Result<(), String> {
        joined(self.restore_each(ids).into_iter().filter_map(Result::err).collect())
    }

    /// Puts each item `ids` names back, as [`Trash::restore`]: where each
    /// went, or why it stayed.
    pub fn restore_each(&self, ids: &[String]) -> Vec<Result<PathBuf, String>> {
        let roots = self.roots();
        ids.iter()
            .map(|id| {
                let found = find(&roots, id)?;
                let (original, _) = read_info(&found.info, &found.root)?;
                if let Ok(there) = original.symlink_metadata() {
                    return Err(taken(&original, there.is_dir()));
                }
                if let Some(folder) = original.parent() {
                    std::fs::create_dir_all(folder).map_err(|e| failed("restore", &original, &e))?;
                }
                rename_new(&found.file(), &original).map_err(|e| match e.raw_os_error() {
                    Some(libc::EEXIST) => taken(&original, original.is_dir()),
                    _ => failed("restore", &original, &e),
                })?;
                // The item is back; a stale info file is only ignored by
                // every app.
                let _ = std::fs::remove_file(&found.info);
                Ok(original)
            })
            .collect()
    }

    /// Deletes each item `ids` names from the trash, for good.
    pub fn erase(&self, ids: &[String]) -> Result<(), String> {
        let roots = self.roots();
        each(ids, |id| {
            let found = find(&roots, id)?;
            erase(&found.file(), &found.info)
        })
    }

    /// Deletes everything in every trash, for good.
    pub fn empty(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        for root in self.roots() {
            let info = root.info();
            for entry in std::fs::read_dir(root.files()).into_iter().flatten().flatten() {
                let name = entry.file_name();
                let mut info_name = name.clone();
                info_name.push(INFO_SUFFIX);
                if let Err(e) = erase(&entry.path(), &info.join(info_name)) {
                    problems.push(e);
                }
            }
            // Info files without an item are left: one may be another
            // app's, trashing its item right now.
            let _ = std::fs::remove_file(root.dir.join(DIRECTORY_SIZES));
        }
        joined(problems)
    }

    /// Every trash this user has now: the home one, then each drive's.
    fn roots(&self) -> Vec<Root> {
        let mut roots: Vec<Root> = self.home.iter().map(|dir| Root { dir: dir.clone(), top: None }).collect();
        let tops = std::fs::read_to_string(&self.mounts).map(|table| disk_mounts(&table)).unwrap_or_default();
        for top in tops {
            for dir in [self.shared_trash(&top), Some(self.user_trash(&top))].into_iter().flatten() {
                let root = Root { dir, top: Some(top.clone()) };
                if self.is_ours(&root.dir) && !roots.iter().any(|known| known.dir == root.dir) {
                    roots.push(root);
                }
            }
        }
        roots
    }

    /// `$top/.Trash/$uid`, when `$top/.Trash` is a real sticky folder, as
    /// the spec asks before using it.
    fn shared_trash(&self, top: &Path) -> Option<PathBuf> {
        let shared = top.join(SHARED_TRASH);
        let meta = shared.symlink_metadata().ok()?;
        (meta.is_dir() && meta.mode() & libc::S_ISVTX != 0).then(|| shared.join(self.uid.to_string()))
    }

    fn user_trash(&self, top: &Path) -> PathBuf {
        top.join(format!("{USER_TRASH_PREFIX}{}", self.uid))
    }

    /// `dir` is a real folder of this user's, and so are its `files` and
    /// `info` where they exist: never a link someone else put on a shared
    /// stick, which emptying the trash would follow.
    fn is_ours(&self, dir: &Path) -> bool {
        let owned = |path: &Path| path.symlink_metadata().is_ok_and(|meta| meta.is_dir() && meta.uid() == self.uid);
        owned(dir)
            && [FILES, INFO].iter().all(|child| {
                let path = dir.join(child);
                path.symlink_metadata().is_err() || owned(&path)
            })
    }

    /// Trashes `asked`, answering its id.
    fn trash_one(&self, asked: &Path) -> Result<String, String> {
        let meta = asked.symlink_metadata().map_err(|e| failed("move to the trash", asked, &e))?;
        // Its folder as the mount table spells it (`~/Stick` may be a link
        // to `/run/media/…`), so its drive's top is found; the item itself
        // stays as named, so a link goes to the trash, not what it points to.
        let name = asked.file_name().ok_or_else(|| format!("{} can't be moved to the trash", asked.display()))?;
        let folder = asked.parent().unwrap_or(asked);
        let path = &folder.canonicalize().map_err(|e| failed("move to the trash", asked, &e))?.join(name);
        let root = self.root_for(path, meta.dev())?;
        if root.dir.starts_with(path) || path.starts_with(&root.dir) {
            return Err(format!("{} holds the trash", path.display()));
        }
        let original = match &root.top {
            Some(top) => path.strip_prefix(top).unwrap_or(path),
            None => path,
        };
        let date = jiff::Zoned::now().strftime(DATE_FORMAT).to_string();
        let contents = format!("{INFO_GROUP}\n{PATH_KEY}{}\n{DATE_KEY}{date}\n", escape(original.as_os_str()));
        let (stored, info) = claim(&root, name, &contents).map_err(|e| failed("move to the trash", path, &e))?;
        if let Err(e) = std::fs::rename(path, root.files().join(stored)) {
            let _ = std::fs::remove_file(&info);
            return Err(failed("move to the trash", path, &e));
        }
        Ok(escape(info.as_os_str()))
    }

    /// The trash for an item on drive `dev`, made if it isn't there yet.
    fn root_for(&self, path: &Path, dev: u64) -> Result<Root, String> {
        if let Some(home) = &self.home {
            make_trash(home).map_err(|e| failed("make the trash for", path, &e))?;
            if home.metadata().is_ok_and(|meta| meta.dev() == dev) {
                return Ok(Root { dir: home.clone(), top: None });
            }
        }
        let top = top_of(path, dev);
        // A trash on a drive the mount table doesn't list (tmpfs, a
        // subvolume) would never be listed, so its items would be lost.
        let tops = std::fs::read_to_string(&self.mounts).map(|table| disk_mounts(&table)).unwrap_or_default();
        if !tops.contains(&top) {
            return Err(format!("{} can't go to the trash: its drive has no trash", path.display()));
        }
        if let Some(dir) = self.shared_trash(&top)
            && make_trash(&dir).is_ok()
            && self.is_ours(&dir)
        {
            return Ok(Root { dir, top: Some(top) });
        }
        let dir = self.user_trash(&top);
        match make_trash(&dir) {
            Ok(()) if self.is_ours(&dir) => Ok(Root { dir, top: Some(top) }),
            Ok(()) => Err(format!("{} isn't a trash of yours", dir.display())),
            Err(e) => Err(failed("make a trash on the drive of", path, &e)),
        }
    }

    /// The items in `root`: each info file with its item still there.
    fn items(&self, root: &Root) -> Vec<TrashItem> {
        let files = root.files();
        let mime = self.mime.current();
        let Ok(found) = std::fs::read_dir(root.info()) else { return Vec::new() };
        found
            .flatten()
            .filter_map(|entry| {
                let info = entry.path();
                let name = stored_of(info.file_name()?)?;
                let (original, deleted_ms) = read_info(&info, root).ok()?;
                let mut entry = folder::read_entry(&files, OsStr::from_bytes(name), &mime)?;
                let shown = original.file_name().unwrap_or(original.as_os_str());
                entry.name = wire_text(shown);
                // The item may be named `name.2` in the trash; its type is
                // its original name's.
                if !entry.dir
                    && let Ok(metadata) = files.join(OsStr::from_bytes(name)).symlink_metadata()
                {
                    entry.mime = mime.type_by_name(&shown.to_string_lossy(), &metadata).to_owned();
                }
                entry.hidden = folder::is_hidden(&entry.name);
                let folder = wire_text(original.parent().unwrap_or(Path::new("/")));
                Some(TrashItem { id: escape(info.as_os_str()), folder, deleted_ms, entry })
            })
            .collect()
    }
}

/// Follows every trash and publishes `files_trash_changed` when one
/// changes, until stopped.
pub struct Watch {
    stopper: Stopper,
}

impl Watch {
    pub fn start(trash: Arc<Trash>, hub: Arc<Hub>, logger: Arc<Logger>) -> Option<Self> {
        let watcher = match Inotify::new(inotify::ENTRIES | inotify::ITSELF) {
            Ok(watcher) => watcher,
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("Can't follow the trash: {e}"));
                return None;
            }
        };
        let stopper = watcher.stopper();
        // Made now, as every desktop does, so it's followed from the first
        // item on; a drive's trash made later is found by the next rescan.
        if let Some(home) = &trash.home
            && let Err(e) = make_trash(home)
        {
            logger.warn(LOG_SOURCE, &format!("Can't make the trash at {}: {e}", home.display()));
        }
        let spawned =
            std::thread::Builder::new().name("novad-files-trash".into()).spawn(move || follow(&trash, watcher, &hub));
        match spawned {
            Ok(_) => Some(Self { stopper }),
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("Can't follow the trash: {e}"));
                None
            }
        }
    }

    pub fn stop(&self) {
        self.stopper.stop();
    }
}

fn follow(trash: &Trash, watcher: Inotify, hub: &Hub) {
    let mut watched: Vec<PathBuf> = Vec::new();
    loop {
        // Trashes that came or went since (a drive plugged in, a drive's
        // first trashed item): follow them, and say so, since their items
        // come or go from the list.
        // Info files and items both, as another app may move an item in
        // well after its info file.
        let infos: Vec<PathBuf> = trash.roots().iter().flat_map(|root| [root.info(), root.files()]).collect();
        let mut changed = false;
        for gone in watched.iter().filter(|dir| !infos.contains(dir)) {
            watcher.unwatch(gone);
            changed = true;
        }
        watched.retain(|dir| infos.contains(dir));
        for info in infos {
            if !watched.contains(&info) && watcher.watch(&info).is_ok() {
                watched.push(info);
                changed = true;
            }
        }
        if changed {
            hub.publish(&Event::FilesTrashChanged);
        }
        let Ok(Some(changes)) = watcher.read_until(Some(Instant::now() + RESCAN)) else { return };
        if changes.is_empty() {
            continue;
        }
        watched.retain(|dir| !changes.gone.contains(dir));
        // Wait for the trash to go quiet.
        loop {
            match watcher.read_until(Some(Instant::now() + QUIET)) {
                Ok(Some(more)) if more.is_empty() => break,
                Ok(Some(more)) => watched.retain(|dir| !more.gone.contains(dir)),
                _ => return,
            }
        }
        hub.publish(&Event::FilesTrashChanged);
    }
}

/// `path` from the socket, once it's absolute, short enough and has no
/// `.` or `..` in it.
fn checked(path: &str) -> Result<PathBuf, String> {
    if path.len() > FILES_PATH_MAX_WIRE_BYTES || path.contains('\0') {
        return Err("Not a path".into());
    }
    super::target::clean(path)
}

/// Runs `work` on each of `items`, then reports every failure at once.
fn each(items: &[String], work: impl Fn(&str) -> Result<(), String>) -> Result<(), String> {
    joined(items.iter().filter_map(|item| work(item).err()).collect())
}

fn joined(problems: Vec<String>) -> Result<(), String> {
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

fn failed(doing: &str, path: &Path, error: &std::io::Error) -> String {
    let shown = path.display();
    match error.kind() {
        std::io::ErrorKind::NotFound => format!("{shown} doesn't exist"),
        std::io::ErrorKind::PermissionDenied => format!("No permission to {doing} {shown}"),
        _ => format!("Can't {doing} {shown}: {error}"),
    }
}

/// Why an item can't go back to `original`: something named like it is
/// there now, a folder when `dir`. Windows' words, with the folder's name.
fn taken(original: &Path, dir: bool) -> String {
    let folder = original.parent().and_then(Path::file_name).unwrap_or(OsStr::new("/")).to_string_lossy();
    let kind = if dir { "folder" } else { "file" };
    format!("A {kind} with this name already exists in {folder}.")
}

/// Makes `dir` a trash with its `files/` and `info/`, private, if it
/// isn't one yet.
fn make_trash(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(PRIVATE_DIR_MODE);
    builder.create(dir.join(FILES))?;
    builder.create(dir.join(INFO))
}

/// A name in `root` for an item named `name`, claimed by writing its info
/// file with `contents`: the name and the info file's path.
/// The stored item an info file named `info_name` stands for: never empty,
/// `.` or `..`, which would name the trash's `files` folder or the trash
/// itself (a crafted drive's `...trashinfo`).
fn stored_of(info_name: &OsStr) -> Option<&[u8]> {
    let name = info_name.as_bytes().strip_suffix(INFO_SUFFIX.as_bytes())?;
    (!matches!(name, b"" | b"." | b"..")).then_some(name)
}

/// `name`, cut short (at a character, when it's UTF-8) so its info file's
/// name, with a `.N` for a taken name, still fits in a folder entry. The
/// full name is kept in the info file's `Path`.
fn stored_name(name: &OsStr) -> OsString {
    let most = NAME_MAX_BYTES - INFO_SUFFIX.len() - NAME_TRY_ROOM;
    let bytes = name.as_bytes();
    if bytes.len() <= most {
        return name.to_owned();
    }
    let mut end = most;
    if let Ok(text) = std::str::from_utf8(bytes) {
        while !text.is_char_boundary(end) {
            end -= 1;
        }
    }
    OsStr::from_bytes(&bytes[..end]).to_owned()
}

fn claim(root: &Root, name: &OsStr, contents: &str) -> std::io::Result<(OsString, PathBuf)> {
    for attempt in 1..=MOST_NAME_TRIES {
        let mut stored = stored_name(name);
        if attempt > 1 {
            stored.push(format!(".{attempt}"));
        }
        let mut info_name = stored.clone();
        info_name.push(INFO_SUFFIX);
        let info = root.info().join(info_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true).mode(PRIVATE_FILE_MODE);
        let mut file = match options.open(&info) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        // An item left without its info file holds the name too.
        if root.files().join(&stored).symlink_metadata().is_ok() {
            drop(file);
            let _ = std::fs::remove_file(&info);
            continue;
        }
        if let Err(e) = file.write_all(contents.as_bytes()) {
            let _ = std::fs::remove_file(&info);
            return Err(e);
        }
        return Ok((stored, info));
    }
    Err(std::io::Error::other("every name is taken in the trash"))
}

/// The highest folder above `path` still on drive `dev`: the drive's top.
fn top_of(path: &Path, dev: u64) -> PathBuf {
    let mut top = path.parent().unwrap_or(path).to_path_buf();
    while let Some(up) = top.parent() {
        if !up.metadata().is_ok_and(|meta| meta.dev() == dev) {
            break;
        }
        top = up.to_path_buf();
    }
    top
}

/// The mount points of local disks in a `/proc/self/mountinfo` table.
fn disk_mounts(table: &str) -> Vec<PathBuf> {
    table
        .lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let point = before.split(' ').nth(4)?;
            let kind = after.split(' ').next()?;
            DISK_FILESYSTEMS.contains(&kind).then(|| PathBuf::from(OsString::from_vec(unescape_octal(point))))
        })
        .collect()
}

/// A mountinfo field with its `\040`-style escapes decoded.
fn unescape_octal(field: &str) -> Vec<u8> {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let code =
            bytes.get(at + 1..at + 4).and_then(|digits| u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok());
        match (bytes[at], code) {
            (b'\\', Some(code)) => {
                out.push(code);
                at += 4;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// Where an info file says its item came from, made absolute, and when
/// it was trashed, in milliseconds since 1970.
fn read_info(info: &Path, root: &Root) -> Result<(PathBuf, Option<i64>), String> {
    let text = std::fs::read_to_string(info).map_err(|e| failed("read", info, &e))?;
    let mut in_group = false;
    let (mut path, mut date) = (None, None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == INFO_GROUP;
        } else if in_group && let Some(value) = line.strip_prefix(PATH_KEY) {
            path = percent_decoded(value).map(|bytes| PathBuf::from(OsString::from_vec(bytes)));
        } else if in_group && let Some(value) = line.strip_prefix(DATE_KEY) {
            date = deletion_ms(value);
        }
    }
    let path =
        path.filter(|path| !path.as_os_str().is_empty()).ok_or_else(|| format!("{} is broken", info.display()))?;
    let path = match (&root.top, path.is_absolute()) {
        (Some(top), false) => top.join(path),
        (None, false) => return Err(format!("{} is broken", info.display())),
        (_, true) => path,
    };
    Ok((path, date))
}

/// A `DeletionDate` (local time) in milliseconds since 1970.
fn deletion_ms(value: &str) -> Option<i64> {
    let local = jiff::civil::DateTime::strptime(DATE_FORMAT, value).ok()?;
    Some(local.to_zoned(jiff::tz::TimeZone::system()).ok()?.timestamp().as_millisecond())
}

/// `bytes` percent-escaped as a URI path: an info file's `Path`, or a
/// bookmark's `file://` URI after its scheme.
pub(super) fn escape(bytes: &OsStr) -> String {
    crate::notifications::image::percent_encoded(bytes.as_bytes(), PATH_SAFE)
}

/// The item `id` names, once it's an info file in one of `roots`.
fn find(roots: &[Root], id: &str) -> Result<Found, String> {
    let gone = || "That item is no longer in the trash".to_string();
    let info = PathBuf::from(OsString::from_vec(percent_decoded(id).ok_or_else(gone)?));
    let root = roots.iter().find(|root| info.parent() == Some(root.info().as_path())).ok_or_else(gone)?;
    let name = info.file_name().and_then(stored_of).ok_or_else(gone)?;
    if !info.is_file() {
        return Err(gone());
    }
    Ok(Found { root: root.clone(), name: OsStr::from_bytes(name).to_owned(), info })
}

/// Deletes a trashed item for good, then its info file.
fn erase(file: &Path, info: &Path) -> Result<(), String> {
    match remove(file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed("delete", file, &e)),
    }
    let _ = std::fs::remove_file(info);
    Ok(())
}

/// Deletes `path`: a folder with all it holds, a link but not what it
/// points to.
fn remove(path: &Path) -> std::io::Result<()> {
    if path.symlink_metadata()?.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

/// Renames `from` to `to`, failing with EEXIST when `to` is there, even
/// if it came after a check.
pub(super) fn rename_new(from_path: &Path, to_path: &Path) -> std::io::Result<()> {
    let from = std::ffi::CString::new(from_path.as_os_str().as_bytes())?;
    let to = std::ffi::CString::new(to_path.as_os_str().as_bytes())?;
    // SAFETY: both paths are NUL-terminated C strings that outlive the call.
    let done =
        unsafe { libc::renameat2(libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) };
    if done == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // A filesystem without RENAME_NOREPLACE (some network ones): the check
    // made before is all there is.
    if error.raw_os_error() == Some(libc::EINVAL) {
        return std::fs::rename(from_path, to_path);
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::LogLevel;
    use nova_test_support::TempDir;

    /// A home trash in a temp folder, with no drives.
    struct Fixture {
        dir: TempDir,
        trash: Trash,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            Self::with_mounts(name, "")
        }

        fn with_mounts(name: &str, table: &str) -> Self {
            let dir = TempDir::new(&format!("files-trash-{name}"));
            std::fs::create_dir_all(dir.join("home/Documents")).unwrap();
            std::fs::write(dir.join("mountinfo"), table).unwrap();
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::default()).unwrap());
            let mime = Mime::follow(Vec::new(), logger);
            let trash = Trash::new(Some(dir.join("home/.local/share/Trash")), dir.join("mountinfo"), mime);
            Self { dir, trash }
        }

        /// Trashes `paths`, saying which didn't go.
        fn trash_all(&self, paths: &[String]) -> Result<(), String> {
            joined(self.trash.trash_each(paths).into_iter().filter_map(Result::err).collect())
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.join(name)
        }

        fn text(&self, name: &str) -> String {
            self.path(name).display().to_string()
        }

        fn write(&self, name: &str) -> String {
            std::fs::write(self.path(name), name).unwrap();
            self.text(name)
        }
    }

    #[test]
    fn a_name_too_long_for_its_info_file_is_cut_short_in_the_trash() {
        let fixture = Fixture::new("long");
        let long = "é".repeat(123);
        let file = fixture.write(&format!("home/Documents/{long}"));
        fixture.trash_all(std::slice::from_ref(&file)).unwrap();
        let items = fixture.trash.list();
        assert_eq!(items[0].entry.name, long, "the full name comes from the info file");
        fixture.trash.restore(std::slice::from_ref(&items[0].id)).unwrap();
        assert!(Path::new(&file).exists());
    }

    #[test]
    fn an_item_in_a_linked_folder_goes_to_the_trash_itself() {
        let fixture = Fixture::new("linked");
        std::os::unix::fs::symlink(fixture.path("home/Documents"), fixture.path("home/Linked")).unwrap();
        fixture.write("home/Documents/a.txt");
        let through_link = fixture.text("home/Linked/a.txt");
        fixture.trash_all(std::slice::from_ref(&through_link)).unwrap();
        assert!(!fixture.path("home/Documents/a.txt").exists());
        assert!(fixture.path("home/Linked").exists(), "the link stays");
        fixture.trash.restore(&[fixture.trash.list()[0].id.clone()]).unwrap();
        assert!(fixture.path("home/Documents/a.txt").exists());
    }

    #[test]
    fn a_trashed_file_follows_the_spec_and_lists_where_it_came_from() {
        let fixture = Fixture::new("spec");
        let report = fixture.write("home/Documents/report 1.txt");
        fixture.trash_all(std::slice::from_ref(&report)).unwrap();

        assert!(!Path::new(&report).exists());
        let trash = fixture.path("home/.local/share/Trash");
        assert_eq!(std::fs::read_to_string(trash.join("files/report 1.txt")).unwrap(), "home/Documents/report 1.txt");
        let info = std::fs::read_to_string(trash.join("info/report 1.txt.trashinfo")).unwrap();
        let documents = escape(fixture.path("home/Documents").as_os_str());
        assert!(info.starts_with(&format!("[Trash Info]\nPath={documents}/report%201.txt\nDeletionDate=")), "{info}");

        let items = fixture.trash.list();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entry.name, "report 1.txt");
        assert_eq!(items[0].folder, fixture.text("home/Documents"));
        let deleted = items[0].deleted_ms.unwrap();
        assert!((jiff::Timestamp::now().as_millisecond() - deleted).abs() < 5_000, "{deleted}");
    }

    #[test]
    fn the_same_name_twice_gets_a_number_and_both_come_back() {
        let fixture = Fixture::new("twice");
        let note = fixture.write("home/note");
        fixture.trash_all(std::slice::from_ref(&note)).unwrap();
        std::fs::create_dir(fixture.path("home/note")).unwrap();
        fixture.trash_all(std::slice::from_ref(&note)).unwrap();
        assert!(fixture.path("home/.local/share/Trash/files/note.2").is_dir());

        let items = fixture.trash.list();
        let folder = items.iter().find(|item| item.entry.dir).unwrap();
        fixture.trash.restore(std::slice::from_ref(&folder.id)).unwrap();
        assert!(fixture.path("home/note").is_dir());
        // The file's place is taken now: it stays in the trash.
        let file = &fixture.trash.list()[0];
        let error = fixture.trash.restore(std::slice::from_ref(&file.id)).unwrap_err();
        assert!(error.starts_with("A folder with this name already exists in "), "{error}");
        std::fs::remove_dir(fixture.path("home/note")).unwrap();
        fixture.trash.restore(std::slice::from_ref(&file.id)).unwrap();
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "home/note");
        assert!(fixture.trash.list().is_empty());
    }

    #[test]
    fn restore_makes_a_folder_that_went() {
        let fixture = Fixture::new("folder-went");
        let photo = fixture.write("home/Documents/photo.png");
        fixture.trash_all(std::slice::from_ref(&photo)).unwrap();
        std::fs::remove_dir(fixture.path("home/Documents")).unwrap();
        let id = fixture.trash.list()[0].id.clone();
        fixture.trash.restore(&[id]).unwrap();
        assert!(Path::new(&photo).is_file());
    }

    #[test]
    fn other_apps_items_are_listed_and_broken_ones_left_out() {
        let fixture = Fixture::new("other-apps");
        let trash = fixture.path("home/.local/share/Trash");
        make_trash(&trash).unwrap();
        // As GIO writes it, with an escaped name that isn't UTF-8.
        std::fs::write(trash.join("files/caf\u{e9}"), b"").unwrap();
        std::fs::write(
            trash.join("info/caf\u{e9}.trashinfo"),
            "[Trash Info]\nPath=/home/me/caf%C3%A9\nDeletionDate=2026-11-02T10:00:00\n",
        )
        .unwrap();
        std::fs::write(trash.join(OsStr::from_bytes(b"files/raw\xff")), b"").unwrap();
        std::fs::write(trash.join(OsStr::from_bytes(b"info/raw\xff.trashinfo")), "[Trash Info]\nPath=/tmp/raw%FF\n")
            .unwrap();
        // No item, no path, a relative path in the home trash.
        std::fs::write(trash.join("info/gone.trashinfo"), "[Trash Info]\nPath=/tmp/gone\n").unwrap();
        std::fs::write(trash.join("files/nopath"), b"").unwrap();
        std::fs::write(trash.join("info/nopath.trashinfo"), "[Trash Info]\nDeletionDate=2026-11-02T10:00:00\n")
            .unwrap();
        std::fs::write(trash.join("files/relative"), b"").unwrap();
        std::fs::write(trash.join("info/relative.trashinfo"), "[Trash Info]\nPath=relative\n").unwrap();

        let items = fixture.trash.list();
        let names: Vec<&str> = items.iter().map(|item| item.entry.name.as_str()).collect();
        assert_eq!(names, ["café".to_string(), wire_text(OsStr::from_bytes(b"raw\xff"))]);
        let expected = jiff::civil::date(2026, 11, 2).at(10, 0, 0, 0).to_zoned(jiff::tz::TimeZone::system()).unwrap();
        assert_eq!(items[0].deleted_ms, Some(expected.timestamp().as_millisecond()));
        // A name that isn't UTF-8 still erases by its id.
        fixture.trash.erase(std::slice::from_ref(&items[1].id)).unwrap();
        assert!(!trash.join(OsStr::from_bytes(b"files/raw\xff")).exists());
    }

    #[test]
    fn ids_outside_a_trash_are_refused() {
        let fixture = Fixture::new("ids");
        let secret = fixture.write("home/secret");
        let restore = |id: &str| fixture.trash.restore(&[id.to_string()]).unwrap_err();
        assert!(restore(&escape(OsStr::new(&secret))).contains("no longer"));
        let outside = fixture.text("home/.local/share/Trash/info/../../secret.trashinfo");
        assert!(restore(&outside).contains("no longer"));
        assert!(restore("%zz").contains("no longer"));
        assert!(Path::new(&secret).exists());
    }

    #[test]
    fn the_trash_itself_and_relative_paths_cant_be_trashed() {
        let fixture = Fixture::new("itself");
        let home = fixture.text("home");
        assert!(fixture.trash_all(std::slice::from_ref(&home)).unwrap_err().contains("holds the trash"));
        let inside = fixture.text("home/.local/share/Trash/files");
        assert!(fixture.trash_all(&[inside]).unwrap_err().contains("holds the trash"));
        assert!(fixture.trash_all(&["home/Documents".into()]).is_err());
        assert!(fixture.trash_all(&[format!("{home}/Documents/..")]).is_err());
        assert!(fixture.path("home/Documents").is_dir());
    }

    #[test]
    fn erase_and_empty_delete_for_good() {
        let fixture = Fixture::new("empty");
        std::fs::create_dir_all(fixture.path("home/tree/deep")).unwrap();
        let deep = fixture.write("home/tree/deep/file");
        let a = fixture.write("home/a");
        let b = fixture.write("home/b");
        fixture.trash_all(&[fixture.text("home/tree"), a, b]).unwrap();
        let items = fixture.trash.list();
        assert_eq!(items.len(), 3);
        let a_id = items.iter().find(|item| item.entry.name == "a").unwrap().id.clone();
        fixture.trash.erase(&[a_id]).unwrap();
        assert_eq!(fixture.trash.list().len(), 2);
        let trash = fixture.path("home/.local/share/Trash");
        std::fs::write(trash.join(DIRECTORY_SIZES), "").unwrap();
        fixture.trash.empty().unwrap();
        assert!(fixture.trash.list().is_empty());
        assert_eq!(std::fs::read_dir(trash.join(FILES)).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(trash.join(INFO)).unwrap().count(), 0);
        assert!(!trash.join(DIRECTORY_SIZES).exists());
        assert!(!Path::new(&deep).exists());
    }

    #[test]
    fn delete_removes_folders_and_links_but_not_what_links_point_to() {
        let fixture = Fixture::new("delete");
        let kept = fixture.write("home/Documents/kept");
        std::os::unix::fs::symlink(&kept, fixture.path("home/link")).unwrap();
        std::fs::create_dir_all(fixture.path("home/old/inner")).unwrap();
        Trash::delete(&[fixture.text("home/link"), fixture.text("home/old")]).unwrap();
        assert!(!fixture.path("home/link").exists() && !fixture.path("home/old").exists());
        assert!(Path::new(&kept).exists());
        assert!(Trash::delete(&[fixture.text("home/missing")]).unwrap_err().contains("doesn't exist"));
    }

    #[test]
    fn a_drive_trash_is_found_from_the_mount_table_with_relative_paths() {
        let table = "36 35 98:0 / /media/stick rw - vfat /dev/sdb1 rw\n37 35 0:5 / /proc rw - proc proc rw\n";
        let tops = disk_mounts(&format!("{table}38 35 8:1 / /mnt/My\\040Disk rw - ext4 /dev/sda1 rw\n"));
        assert_eq!(tops, [PathBuf::from("/media/stick"), PathBuf::from("/mnt/My Disk")]);

        // A drive whose top is a folder in the fixture.
        let fixture = Fixture::new("drive");
        let top = fixture.path("stick");
        let mountinfo = format!("40 35 8:17 / {} rw - exfat /dev/sdc1 rw\n", top.display());
        std::fs::write(fixture.path("mountinfo"), mountinfo).unwrap();
        let root = Root { dir: fixture.trash.user_trash(&top), top: Some(top.clone()) };
        make_trash(&root.dir).unwrap();
        std::fs::write(root.files().join("song.flac"), b"").unwrap();
        std::fs::write(root.info().join("song.flac.trashinfo"), "[Trash Info]\nPath=Music/song.flac\n").unwrap();
        let items = fixture.trash.list();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].folder, top.join("Music").display().to_string());
        fixture.trash.restore(std::slice::from_ref(&items[0].id)).unwrap();
        assert!(top.join("Music/song.flac").is_file());
    }

    #[test]
    fn a_drive_trash_whose_files_are_a_link_is_never_used() {
        let fixture = Fixture::new("crafted");
        let top = fixture.path("stick");
        let mountinfo = format!("40 35 8:17 / {} rw - exfat /dev/sdc1 rw\n", top.display());
        std::fs::write(fixture.path("mountinfo"), mountinfo).unwrap();
        let elsewhere = fixture.path("home/Documents");
        std::fs::write(elsewhere.join("keep.txt"), b"").unwrap();
        let dir = fixture.trash.user_trash(&top);
        std::fs::create_dir_all(dir.join(INFO)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join(FILES)).unwrap();
        std::fs::write(dir.join(INFO).join("keep.txt.trashinfo"), "[Trash Info]\nPath=keep.txt\n").unwrap();

        assert!(fixture.trash.list().is_empty());
        fixture.trash.empty().unwrap();
        assert!(elsewhere.join("keep.txt").exists(), "emptying never follows the link");
    }

    #[test]
    fn info_files_for_dot_names_are_never_items() {
        let fixture = Fixture::new("dots");
        let trash = fixture.path("home/.local/share/Trash");
        make_trash(&trash).unwrap();
        std::fs::write(trash.join("files/kept.txt"), b"").unwrap();
        std::fs::write(trash.join("info/kept.txt.trashinfo"), "[Trash Info]\nPath=/tmp/kept.txt\n").unwrap();
        for dots in ["..", "."] {
            std::fs::write(trash.join(format!("info/{dots}.trashinfo")), "[Trash Info]\nPath=/tmp/x\n").unwrap();
        }
        let items = fixture.trash.list();
        assert_eq!(items.len(), 1, "only kept.txt");
        let dots = escape(trash.join("info/...trashinfo").as_os_str());
        assert!(fixture.trash.erase(&[dots]).unwrap_err().contains("no longer"));
        assert!(trash.join("files/kept.txt").exists());
    }
}
