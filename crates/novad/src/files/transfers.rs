//! Copy and move jobs (`files_transfer`): each client's running jobs, one
//! thread each, reporting to the client that started them.
//!
//! A job first checks its items and measures them ([`copying::measure`]),
//! then places them one by one, depth first. An item copied into the
//! folder it's already in gets a free numbered name; one moved there stays
//! put. A name that's taken asks the client with a [`Clash`] and waits for
//! `files_transfer_answer` (or the same answer given to all clashes of its
//! kind earlier). A file replaces a file; a folder merges into a folder,
//! asking about each clash inside. Files are written under a temporary
//! name and renamed into place once complete, so a cancelled or failed
//! copy leaves no half file, and a replaced file is swapped in one step.
//! A move is a rename when it stays on one file system, and a copy and
//! delete when it doesn't.
//!
//! A job may have several legs, each items going into one folder: an undone
//! move puts items back in the folders they came from. As it ends, the job
//! tells the history ([`History::moved`]) each item it placed at the top
//! (not what's inside a folder it copied), so the step can be undone.
//!
//! The job checks for a cancel before each item and between chunks of a
//! file. Every event is sent while the job is still registered, checked
//! under the jobs lock, so nothing arrives after it ended, was cancelled
//! by the client's hang-up or the module stopped; the last one (done,
//! cancelled or failed) unregisters it in the same step.

use crate::files::archive;
use crate::files::copying::{self, Aside, Clashing, Measure, Part};
use crate::files::history::{History, Origin};
use crate::files::views::checked_path;
use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use nova_protocol::{
    Clash, ClashAnswer, Event, FILES_MAX_TRANSFERS, FILES_TRANSFER_MAX_SOURCES, FILES_TRANSFER_PROGRESS_MS, Transfer,
    TransferMode, TransferState,
};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const LOG_SOURCE: &str = "files";
const PROGRESS_EVERY: Duration = Duration::from_millis(FILES_TRANSFER_PROGRESS_MS);
/// A new zip's permissions, as an app saving a file would give it, kept
/// to what its items allow: a zip of private files stays private.
const ZIP_MODE: u32 = 0o644;

/// A job, named by its client and the client's number for it.
type Key = (ClientId, u32);

/// The running jobs; `None` once the module stopped, so a `files_transfer`
/// already on its way can't start one after that.
type Jobs = Arc<Mutex<Option<HashMap<Key, Arc<Job>>>>>;

/// The copy and move jobs clients run.
pub struct Transfers {
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    history: Arc<History>,
    jobs: Jobs,
}

/// Items going into one folder.
pub struct Leg {
    pub sources: Vec<PathBuf>,
    pub folder: PathBuf,
}

struct Job {
    key: Key,
    mode: TransferMode,
    /// How many items it was given, and the first one's name.
    items: u32,
    name: String,
    control: Mutex<Control>,
    /// Signalled when `control` changes.
    changed: Condvar,
}

/// What the client tells a job while it runs.
#[derive(Default)]
struct Control {
    stopped: bool,
    /// The clash it waits on: its id, and whether it's a folder clash.
    asking: Option<(u32, bool)>,
    /// The answer to it, and whether it goes to the rest of its kind.
    answer: Option<(ClashAnswer, bool)>,
}

impl Transfers {
    pub fn new(hub: Arc<Hub>, logger: Arc<Logger>, history: Arc<History>) -> Self {
        Self { hub, logger, history, jobs: Arc::new(Mutex::new(Some(HashMap::new()))) }
    }

    /// Starts `client`'s job `job`, copying or moving `sources` into
    /// `folder`: a new step in the history. Doesn't touch the disk: how it
    /// goes follows as events.
    pub fn start(
        &self,
        client: ClientId,
        job: u32,
        mode: TransferMode,
        sources: &[String],
        folder: &str,
    ) -> Result<(), String> {
        let sources = sources.iter().map(|source| checked_path(source)).collect::<Result<Vec<_>, _>>()?;
        let leg = Leg { sources, folder: checked_path(folder)? };
        self.start_legs(client, job, mode, vec![leg], Origin::New)
    }

    /// Starts `client`'s job `job` over `legs`, whose paths are checked,
    /// for `origin`.
    pub fn start_legs(
        &self,
        client: ClientId,
        job: u32,
        mode: TransferMode,
        legs: Vec<Leg>,
        origin: Origin,
    ) -> Result<(), String> {
        let count: usize = legs.iter().map(|leg| leg.sources.len()).sum();
        let Some(first) = legs.iter().find_map(|leg| leg.sources.first()) else {
            return Err("Nothing to copy".into());
        };
        if count > FILES_TRANSFER_MAX_SOURCES {
            return Err(format!("At most {FILES_TRANSFER_MAX_SOURCES} items can be copied at once"));
        }
        let name = first.file_name().map_or_else(|| "/".into(), nova_protocol::wire_text);
        let key = (client, job);
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return Err("Nova Files is off".into()) };
        if running.contains_key(&key) {
            return Err(format!("Job {job} is already running"));
        }
        if running.keys().filter(|(owner, _)| *owner == client).count() >= FILES_MAX_TRANSFERS {
            return Err(format!("At most {FILES_MAX_TRANSFERS} copies can run at once"));
        }
        let items = u32::try_from(count).unwrap_or(u32::MAX);
        let started = Arc::new(Job { key, mode, items, name, control: Mutex::default(), changed: Condvar::new() });
        running.insert(key, Arc::clone(&started));
        drop(jobs);
        let run = Run {
            job: started,
            hub: Arc::clone(&self.hub),
            logger: Arc::clone(&self.logger),
            jobs: Arc::clone(&self.jobs),
            history: Arc::clone(&self.history),
            origin,
            legs,
            placed: Vec::new(),
            inside_copy: 0,
            done_bytes: 0,
            total: Measure::default(),
            remembered: [None; 2],
            next_clash: 0,
            last_report: Instant::now(),
            copying_moved: false,
            staging: None,
            placing_extracted: false,
        };
        let spawned = std::thread::Builder::new().name("novad-files-copy".into()).spawn(move || run.run());
        spawned.map(drop).map_err(|e| {
            self.forget(|key| *key == (client, job));
            format!("Can't start copying: {e}")
        })
    }

    /// Stops `client`'s job `job`, which then reports it was cancelled.
    /// One that already ended is left alone.
    pub fn cancel(&self, client: ClientId, job: u32) {
        if let Some(job) = lock(&self.jobs).as_ref().and_then(|jobs| jobs.get(&(client, job)).cloned()) {
            job.stop();
        }
    }

    /// Answers the clash `clash` that `client`'s job `job` asks about.
    pub fn answer(&self, client: ClientId, job: u32, clash: u32, answer: ClashAnswer, all: bool) -> Result<(), String> {
        let found = lock(&self.jobs).as_ref().and_then(|jobs| jobs.get(&(client, job)).cloned());
        let Some(running) = found else { return Err(format!("Job {job} isn't running")) };
        let mut control = lock(&running.control);
        let Some((_, folder)) = control.asking.filter(|(asking, _)| *asking == clash) else {
            return Err(format!("Job {job} isn't asking about clash {clash}"));
        };
        match (answer, folder) {
            (ClashAnswer::Replace, true) => return Err("A folder can be merged or skipped".into()),
            (ClashAnswer::Merge, false) => return Err("Only folders can be merged".into()),
            _ => {}
        }
        control.answer = Some((answer, all));
        running.changed.notify_all();
        Ok(())
    }

    /// Stops every job of `client`, which hung up.
    pub fn client_gone(&self, client: ClientId) {
        self.forget(|(owner, _)| *owner == client);
    }

    /// Stops every job, for good.
    pub fn stop(&self) {
        for (_, job) in lock(&self.jobs).take().into_iter().flatten() {
            job.stop();
        }
    }

    /// Unregisters and stops the jobs `which` picks; they send nothing more.
    fn forget(&self, which: impl Fn(&Key) -> bool) {
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return };
        running.retain(|key, job| {
            let keep = !which(key);
            if !keep {
                job.stop();
            }
            keep
        });
    }
}

impl Job {
    fn stop(&self) {
        lock(&self.control).stopped = true;
        self.changed.notify_all();
    }

    fn stopped(&self) -> bool {
        lock(&self.control).stopped
    }
}

/// Why a job ended early.
enum Halt {
    Stopped,
    Failed(String),
}

type Step = Result<(), Halt>;

/// An item to place, checked.
struct Item {
    source: PathBuf,
    /// The folder it goes into.
    folder: PathBuf,
    name: OsString,
    meta: Metadata,
    /// Its bytes, as measured.
    bytes: u64,
    /// It's copied into the folder it's in, under a new name.
    renamed: bool,
}

/// The thread that runs one job.
struct Run {
    job: Arc<Job>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    jobs: Jobs,
    history: Arc<History>,
    origin: Origin,
    legs: Vec<Leg>,
    /// Each item placed at the top, (from, to): what the history keeps.
    placed: Vec<(PathBuf, PathBuf)>,
    /// How deep in folders being copied it is; what's placed there isn't
    /// at the top.
    inside_copy: u32,
    done_bytes: u64,
    /// The bytes to copy and the clashes still ahead, kept as items are
    /// placed or skipped.
    total: Measure,
    /// Answers given to all file clashes ([0]) or all folder clashes ([1]).
    remembered: [Option<ClashAnswer>; 2],
    next_clash: u32,
    last_report: Instant,
    /// Set while a folder moved to another file system is copied: its
    /// entries stay in place until all of it is, so a stop halfway (which
    /// may put back what it replaced) loses none of them.
    copying_moved: bool,
    /// The hidden folder in `folder` archives are extracted to, deleted
    /// with whatever is left in it when the job ends.
    staging: Option<PathBuf>,
    /// Extracted items are being placed: their bytes were counted as they
    /// were extracted.
    placing_extracted: bool,
}

impl Run {
    fn run(mut self) {
        self.report(TransferState::Counting, None, None);
        let result = if self.job.mode == TransferMode::Compress {
            self.compress()
        } else {
            self.prepare().and_then(|items| {
                self.report(TransferState::Running, None, None);
                items.into_iter().try_for_each(|item| self.place_item(item))
            })
        };
        self.record();
        if let Some(staging) = self.staging.take()
            && let Err(e) = fs::remove_dir_all(&staging)
        {
            self.logger.warn(LOG_SOURCE, &format!("can't delete {}: {e}", staging.display()));
        }
        match result {
            Ok(()) => self.report(TransferState::Done, None, None),
            Err(Halt::Stopped) => self.report(TransferState::Cancelled, None, None),
            Err(Halt::Failed(error)) => {
                self.logger.warn(LOG_SOURCE, &error);
                self.report(TransferState::Failed, None, Some(error));
            }
        }
    }

    /// Tells the history what the job placed.
    fn record(&self) {
        if !self.placed.is_empty() {
            self.history.moved(self.origin, self.job.mode, &self.placed);
        }
    }

    /// Checks the job's items and measures them.
    fn prepare(&mut self) -> Result<Vec<Item>, Halt> {
        let mut items = Vec::new();
        for leg in 0..self.legs.len() {
            self.prepare_leg(leg, &mut items)?;
        }
        Ok(items)
    }

    /// Checks leg `leg`'s items and measures them, into `items`; an
    /// archive is extracted first and what it holds is placed instead.
    fn prepare_leg(&mut self, leg: usize, items: &mut Vec<Item>) -> Step {
        let asked = self.legs[leg].folder.clone();
        let folder = fs::canonicalize(&asked).map_err(|e| failed(&asked, &e))?;
        if !folder.is_dir() {
            return Err(Halt::Failed(format!("{} isn't a folder", asked.display())));
        }
        let sources = if self.extracts() { self.unpack(leg, &folder)? } else { self.legs[leg].sources.clone() };
        for source in &sources {
            let meta = fs::symlink_metadata(source).map_err(|e| failed(source, &e))?;
            let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
                return Err(Halt::Failed(format!("Can't {} /", self.verb())));
            };
            let in_place = fs::canonicalize(parent).map_err(|e| failed(parent, &e))? == folder;
            if meta.is_dir() && folder.starts_with(fs::canonicalize(source).map_err(|e| failed(source, &e))?) {
                return Err(Halt::Failed(format!("Can't {} {} into itself", self.verb(), name.to_string_lossy())));
            }
            // Moving an item where it already is leaves it be.
            if in_place && self.moves() {
                continue;
            }
            let target = (!in_place).then(|| asked.join(name));
            let job = Arc::clone(&self.job);
            let measured = copying::measure(source, target.as_deref(), &|| job.stopped())
                .map_err(|e| failed(source, &e))?
                .ok_or(Halt::Stopped)?;
            // Extracted bytes were counted as they were extracted.
            let bytes = if self.placing_extracted { 0 } else { measured.bytes };
            self.total.add(Measure { bytes, ..measured });
            let (source, folder, name) = (source.clone(), asked.clone(), name.to_owned());
            items.push(Item { source, folder, name, meta, bytes, renamed: in_place });
        }
        Ok(())
    }

    fn place_item(&mut self, item: Item) -> Step {
        if !item.renamed {
            return self.place(&item.source, &item.folder, &item.name, Some(item.bytes));
        }
        self.check()?;
        let name =
            copying::numbered(&item.folder, &item.name, item.meta.is_dir()).map_err(|e| failed(&item.source, &e))?;
        self.put(&item.source, &item.meta, &item.folder.join(name), None, Some(item.bytes))
    }

    /// Copies or moves `source` into the folder `into` as `name`, asking
    /// when the name is taken. `bytes` is its size when it's known.
    fn place(&mut self, source: &Path, into: &Path, name: &std::ffi::OsStr, bytes: Option<u64>) -> Step {
        self.check()?;
        let meta = fs::symlink_metadata(source).map_err(|e| failed(source, &e))?;
        let target = into.join(name);
        let existing = match fs::symlink_metadata(&target) {
            Ok(existing) => existing,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return self.put(source, &meta, &target, None, bytes),
            Err(e) => return Err(failed(&target, &e)),
        };
        let clashing = Clashing::of(&meta, &existing);
        match self.decide(source, &meta, &target, &existing, clashing)? {
            ClashAnswer::Skip => self.skip(source, &target, clashing),
            ClashAnswer::Merge => self.merge(source, &target),
            ClashAnswer::Replace => {
                if clashing == Clashing::Files {
                    self.total.file_clashes = self.total.file_clashes.saturating_sub(1);
                }
                self.put(source, &meta, &target, Some(&existing), bytes)
            }
        }
    }

    /// What to do about `target` being taken: the answer given to all
    /// clashes of its kind, or the client's.
    fn decide(
        &mut self,
        source: &Path,
        meta: &Metadata,
        target: &Path,
        existing: &Metadata,
        clashing: Clashing,
    ) -> Result<ClashAnswer, Halt> {
        // A folder against a file is never answered for all.
        let kind = match clashing {
            Clashing::Files => Some(0),
            Clashing::Folders => Some(1),
            Clashing::Mixed => None,
        };
        if let Some(answer) = kind.and_then(|kind| self.remembered[kind]) {
            return Ok(answer);
        }
        self.next_clash += 1;
        let id = self.next_clash;
        let folders = clashing == Clashing::Folders;
        let same_kind = match clashing {
            Clashing::Files => self.total.file_clashes,
            Clashing::Folders => self.total.folder_clashes,
            Clashing::Mixed => 0,
        };
        let clash = Clash {
            id,
            name: nova_protocol::wire_text(target.file_name().unwrap_or_default()),
            folder: folders,
            original: copying::clash_item(target, existing),
            replacement: copying::clash_item(source, meta),
            others: same_kind.saturating_sub(1),
        };
        {
            let mut control = lock(&self.job.control);
            if control.stopped {
                return Err(Halt::Stopped);
            }
            control.asking = Some((id, folders));
            control.answer = None;
        }
        self.report(TransferState::Asking, Some(clash), None);
        let mut control = lock(&self.job.control);
        let (answer, all) = loop {
            if control.stopped {
                return Err(Halt::Stopped);
            }
            if let Some(answered) = control.answer.take() {
                control.asking = None;
                break answered;
            }
            control = self.job.changed.wait(control).unwrap_or_else(std::sync::PoisonError::into_inner);
        };
        drop(control);
        if all && let Some(kind) = kind {
            self.remembered[kind] = Some(answer);
        }
        self.report(TransferState::Running, None, None);
        Ok(answer)
    }

    /// Leaves `source` where it is: its bytes and the clashes inside it
    /// come off what's ahead.
    fn skip(&mut self, source: &Path, target: &Path, clashing: Clashing) -> Step {
        let folders = clashing == Clashing::Folders;
        let job = Arc::clone(&self.job);
        let skipped = copying::measure(source, folders.then_some(target), &|| job.stopped())
            .map_err(|e| failed(source, &e))?
            .ok_or(Halt::Stopped)?;
        // Extracted bytes all count, placed or not.
        if !self.placing_extracted {
            self.total.bytes = self.total.bytes.saturating_sub(skipped.bytes);
        }
        // Measured with what it clashed with, a skipped folder counts the
        // clashes a merge would have met; anything else counts as itself.
        let (files, folders) = match clashing {
            Clashing::Folders => (skipped.file_clashes, skipped.folder_clashes),
            Clashing::Files => (1, 0),
            Clashing::Mixed => (0, 0),
        };
        self.total.file_clashes = self.total.file_clashes.saturating_sub(files);
        self.total.folder_clashes = self.total.folder_clashes.saturating_sub(folders);
        Ok(())
    }

    /// Places the folder `source`'s entries in the folder `target`; a moved
    /// folder goes once everything in it has.
    fn merge(&mut self, source: &Path, target: &Path) -> Step {
        self.total.folder_clashes = self.total.folder_clashes.saturating_sub(1);
        self.place_children(source, target)?;
        if self.moves() {
            // Whatever was skipped stays in it.
            let _ = fs::remove_dir(source);
        }
        Ok(())
    }

    fn place_children(&mut self, source: &Path, target: &Path) -> Step {
        let entries = fs::read_dir(source).map_err(|e| failed(source, &e))?;
        for entry in entries {
            let name = entry.map_err(|e| failed(source, &e))?.file_name();
            self.place(&source.join(&name), target, &name, None)?;
        }
        Ok(())
    }

    /// Puts `source` at `target`, replacing `existing` there. A file over
    /// a file is swapped in one step; a folder for a file, or a file for a
    /// folder, sets the old one aside until the new one is in, and puts it
    /// back if that fails or is cancelled. `bytes` is its size when known.
    fn put(
        &mut self,
        source: &Path,
        meta: &Metadata,
        target: &Path,
        existing: Option<&Metadata>,
        bytes: Option<u64>,
    ) -> Step {
        self.check()?;
        let aside = match existing {
            Some(existing) if existing.is_dir() || meta.is_dir() => {
                Some(Aside::new(target).map_err(|e| failed(target, &e))?)
            }
            _ => None,
        };
        let placed = self.put_free(source, meta, target, bytes);
        if placed.is_ok() && self.inside_copy == 0 {
            self.placed.push((source.to_owned(), target.to_owned()));
        }
        let Some(aside) = aside else { return placed };
        let settled = if placed.is_ok() { aside.discard() } else { aside.restore() };
        if let Err(e) = settled {
            self.logger.warn(LOG_SOURCE, &format!("can't tidy up after replacing {}: {e}", target.display()));
        }
        placed
    }

    /// Puts `source` at `target`, where only a file can be.
    fn put_free(&mut self, source: &Path, meta: &Metadata, target: &Path, bytes: Option<u64>) -> Step {
        let moving = self.moves() && !self.copying_moved;
        if moving {
            match fs::rename(source, target) {
                Ok(()) => {
                    let bytes = bytes.unwrap_or_else(|| moved_bytes(target, meta));
                    self.copied(bytes);
                    return Ok(());
                }
                // Another file system: copied, then deleted.
                Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
                Err(e) => return Err(failed(source, &e)),
            }
        }
        if moving && meta.is_dir() {
            self.copying_moved = true;
            let placed = self.copy(source, meta, target);
            self.copying_moved = false;
            // Once all of it is copied the move has happened: what can't
            // be deleted after that stays at the source too, and nothing
            // placed is undone.
            if placed? && let Err(e) = copying::remove_moved(source) {
                self.logger.warn(LOG_SOURCE, &format!("moved {}, but can't delete all of it: {e}", source.display()));
            }
            return Ok(());
        }
        let placed = self.copy(source, meta, target)?;
        if placed && moving {
            fs::remove_file(source).map_err(|e| failed(source, &e))?;
        }
        Ok(())
    }

    /// Copies `source` to the free `target`. False for what can't be
    /// copied (a device, a socket, a pipe), which is left out.
    fn copy(&mut self, source: &Path, meta: &Metadata, target: &Path) -> Result<bool, Halt> {
        let file_type = meta.file_type();
        if file_type.is_file() {
            let into = target.parent().unwrap_or(Path::new("/"));
            let mut part = Part::create(into).map_err(|e| failed(target, &e))?;
            let filled = part.fill(source, &mut |bytes| {
                self.copied(bytes);
                !self.job.stopped()
            });
            if !filled.map_err(|e| failed(source, &e))? {
                return Err(Halt::Stopped);
            }
            part.finish(meta, target).map_err(|e| failed(target, &e))?;
        } else if file_type.is_symlink() {
            copying::copy_link(source, target).map_err(|e| failed(target, &e))?;
        } else if file_type.is_dir() {
            // Private while it fills, so what the source folder kept from
            // other users isn't open to them meanwhile; its own mode after.
            fs::DirBuilder::new().mode(0o700).create(target).map_err(|e| failed(target, &e))?;
            self.inside_copy += 1;
            let children = self.place_children(source, target);
            self.inside_copy -= 1;
            children?;
            // Best effort, after its entries are in: a read-only folder
            // would refuse them.
            let _ = fs::set_permissions(target, meta.permissions());
            if let (Ok(modified), Ok(folder)) = (meta.modified(), fs::File::open(target)) {
                let _ = folder.set_modified(modified);
            }
        } else {
            self.logger.warn(LOG_SOURCE, &format!("left out {}: not a file, folder or link", source.display()));
            return Ok(false);
        }
        Ok(true)
    }

    fn copied(&mut self, bytes: u64) {
        if self.placing_extracted {
            return;
        }
        self.done_bytes += bytes;
        if self.last_report.elapsed() >= PROGRESS_EVERY {
            self.report(TransferState::Running, None, None);
        }
    }

    fn check(&self) -> Step {
        if self.job.stopped() { Err(Halt::Stopped) } else { Ok(()) }
    }

    fn verb(&self) -> &'static str {
        match self.job.mode {
            TransferMode::Copy => "copy",
            TransferMode::Move => "move",
            TransferMode::Extract | TransferMode::ExtractHere => "extract",
            TransferMode::Compress => "compress",
        }
    }

    /// Its items leave where they were: a move, or what was extracted
    /// leaving the folder it was extracted to.
    fn moves(&self) -> bool {
        matches!(self.job.mode, TransferMode::Move | TransferMode::Extract | TransferMode::ExtractHere)
    }

    fn extracts(&self) -> bool {
        matches!(self.job.mode, TransferMode::Extract | TransferMode::ExtractHere)
    }

    /// Extracts each archive among the sources to its own folder in a
    /// hidden one in `folder`, measuring the progress by it, and answers
    /// what to place in `folder`: each archive's folder ("Extract all"),
    /// or what's in it ("Extract here").
    fn unpack(&mut self, leg: usize, folder: &Path) -> Result<Vec<PathBuf>, Halt> {
        let sources = self.legs[leg].sources.clone();
        let mut archives = Vec::with_capacity(sources.len());
        for source in &sources {
            let kind = source.file_name().and_then(archive::Kind::of_name);
            let Some(kind) = kind.filter(|_| fs::metadata(source).is_ok_and(|meta| meta.is_file())) else {
                return Err(Halt::Failed(format!("{} isn't an archive Nova can open", source.display())));
            };
            self.total.bytes += archive::total(source, kind).map_err(|e| unreadable(source, &e))?;
            archives.push((source.clone(), kind));
        }
        let staging = archive::fresh_folder(folder, OsStr::new("extract")).map_err(|e| failed(folder, &e))?;
        self.staging = Some(staging.clone());
        self.report(TransferState::Running, None, None);
        let mut staged = Vec::new();
        for (number, (source, kind)) in archives.into_iter().enumerate() {
            self.check()?;
            // One folder each, so two archives' files meet only in `folder`,
            // where a clash is asked about.
            let own = staging.join(number.to_string());
            let into = match self.job.mode {
                TransferMode::Extract => own.join(archive::folder_name(source.file_name().unwrap_or_default())),
                _ => own.clone(),
            };
            fs::DirBuilder::new().recursive(true).mode(0o700).create(&into).map_err(|e| failed(&into, &e))?;
            let base = self.done_bytes;
            let job = Arc::clone(&self.job);
            let finished = archive::unpack(&source, kind, &into, &mut |done| {
                self.done_bytes = base.saturating_add(done);
                if self.last_report.elapsed() >= PROGRESS_EVERY {
                    self.report(TransferState::Running, None, None);
                }
                !job.stopped()
            });
            if !finished.map_err(|e| unreadable(&source, &e))? {
                return Err(Halt::Stopped);
            }
            let mut names: Vec<_> = fs::read_dir(&own)
                .and_then(|entries| entries.map(|entry| entry.map(|entry| entry.file_name())).collect())
                .map_err(|e| failed(&own, &e))?;
            names.sort();
            staged.extend(names.into_iter().map(|name| own.join(name)));
        }
        self.done_bytes = self.total.bytes;
        self.placing_extracted = true;
        Ok(staged)
    }

    /// Packs the sources into a new zip in the folder, named after the
    /// first ("report.pdf" gives "report.zip").
    fn compress(&mut self) -> Step {
        // Compressing is started with one leg.
        let Leg { sources, folder: asked } = &self.legs[0];
        let (sources, asked) = (sources.clone(), asked.clone());
        let folder = fs::canonicalize(&asked).map_err(|e| failed(&asked, &e))?;
        for source in &sources {
            // A zip made inside a folder it packs would pack itself.
            if fs::canonicalize(source).is_ok_and(|source| folder.starts_with(source)) {
                let name = source.file_name().unwrap_or_default().to_string_lossy();
                return Err(Halt::Failed(format!("Can't compress {name} into itself")));
            }
            let job = Arc::clone(&self.job);
            let measured = copying::measure(source, None, &|| job.stopped())
                .map_err(|e| failed(source, &e))?
                .ok_or(Halt::Stopped)?;
            self.total.bytes += measured.bytes;
        }
        let first = &sources[0];
        let dir = fs::symlink_metadata(first).is_ok_and(|meta| meta.is_dir());
        let stem = if dir { first.file_name() } else { first.file_stem() };
        let mut name = stem.unwrap_or(OsStr::new("Archive")).to_owned();
        name.push(".zip");
        self.report(TransferState::Running, None, None);
        let mut part = Part::create(&asked).map_err(|e| failed(&asked, &e))?;
        let file = part.file().map_err(|e| failed(&asked, &e))?;
        let sources: Vec<&Path> = sources.iter().map(PathBuf::as_path).collect();
        let job = Arc::clone(&self.job);
        let packed = archive::pack(&sources, file, &mut |bytes| {
            self.copied(bytes);
            !job.stopped()
        });
        if !packed.map_err(|e| failed(&asked.join(&name), &e))? {
            return Err(Halt::Stopped);
        }
        self.check()?;
        let mode = sources
            .iter()
            .fold(ZIP_MODE, |mode, source| fs::metadata(source).map_or(mode, |meta| mode & (meta.mode() | 0o600)));
        let kept = part.keep(&name, mode).map_err(|e| failed(&asked.join(&name), &e))?;
        // Undone like a copy: the zip goes to the trash.
        self.placed.push((asked.join(&name), asked.join(kept)));
        Ok(())
    }

    /// Tells the client how the job is going, while it's still registered;
    /// a last state also unregisters it.
    fn report(&mut self, state: TransferState, clash: Option<Clash>, error: Option<String>) {
        self.last_report = Instant::now();
        let last = matches!(state, TransferState::Done | TransferState::Cancelled | TransferState::Failed);
        let (client, job) = self.job.key;
        let transfer = Transfer {
            job,
            mode: self.job.mode,
            state,
            items: self.job.items,
            name: self.job.name.clone(),
            done_bytes: self.done_bytes.min(self.total.bytes),
            total_bytes: self.total.bytes,
            clash,
            error,
        };
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return };
        if !running.get(&self.job.key).is_some_and(|shown| Arc::ptr_eq(shown, &self.job)) {
            return;
        }
        self.hub.send_to(client, &Event::FilesTransfer(transfer));
        if last {
            running.remove(&self.job.key);
        }
    }
}

/// The bytes of what was just moved to `target` in one rename, for the
/// progress: a file's size, or what a folder holds (0 if it can't be read).
fn moved_bytes(target: &Path, meta: &Metadata) -> u64 {
    if meta.is_dir() {
        copying::measure(target, None, &|| false).ok().flatten().map_or(0, |measured| measured.bytes)
    } else if meta.is_file() {
        meta.len()
    } else {
        0
    }
}

/// Why the archive at `path` couldn't be extracted, for the window.
fn unreadable(path: &Path, error: &io::Error) -> Halt {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    Halt::Failed(match error.kind() {
        io::ErrorKind::NotFound => format!("{} doesn't exist", path.display()),
        io::ErrorKind::PermissionDenied => format!("No permission for {}", path.display()),
        io::ErrorKind::StorageFull => format!("No space left to extract {name}"),
        _ => format!("Can't extract {name}: {error}"),
    })
}

/// Whether every item at `sources` is on the same drive (file system) as
/// `folder` (`files_same_drive`), so a drop there moves rather than copies.
/// An item is looked at itself, not through a link, and a drive mounted on
/// a folder counts as its own: dragging a mount point copies it.
pub fn same_drive(sources: &[String], folder: &str) -> Result<bool, String> {
    if sources.len() > FILES_TRANSFER_MAX_SOURCES {
        return Err(format!("At most {FILES_TRANSFER_MAX_SOURCES} items can be copied at once"));
    }
    let device =
        |path: &Path, meta: io::Result<Metadata>| meta.map(|meta| meta.dev()).map_err(|e| why_failed(path, &e));
    let folder = checked_path(folder)?;
    let drive = device(&folder, fs::metadata(&folder))?;
    for source in sources {
        let source = checked_path(source)?;
        if device(&source, fs::symlink_metadata(&source))? != drive {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Why `path` couldn't be copied or moved, for the window.
fn failed(path: &Path, error: &io::Error) -> Halt {
    Halt::Failed(why_failed(path, error))
}

/// Why `path` couldn't be read, copied or moved, in words.
fn why_failed(path: &Path, error: &io::Error) -> String {
    let path = path.display();
    match error.kind() {
        io::ErrorKind::NotFound => format!("{path} doesn't exist"),
        io::ErrorKind::PermissionDenied => format!("No permission for {path}"),
        io::ErrorKind::StorageFull => format!("No space left for {path}"),
        io::ErrorKind::ReadOnlyFilesystem => format!("{path} is on a read-only drive"),
        _ => format!("Can't copy {path}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing, next_named};
    use nova_protocol::LogLevel;
    use nova_test_support::TempDir;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(5);

    /// One client's jobs, between a `from` and a `to` folder.
    struct Fixture {
        dir: TempDir,
        transfers: Transfers,
        client: ClientId,
        events: Receiver<Outgoing>,
        _socket: UnixStream,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("transfers-{name}"));
            fs::create_dir_all(dir.join("from")).unwrap();
            fs::create_dir_all(dir.join("to")).unwrap();
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let (socket, ours) = UnixStream::pair().unwrap();
            let (queue, events) = ClientQueue::new(&ours).unwrap();
            let client = hub.register(queue);
            let history = Arc::new(History::new(Arc::clone(&hub)));
            Self { transfers: Transfers::new(hub, logger, history), dir, client, events, _socket: socket }
        }

        fn write(&self, path: &str, content: &str) {
            let path = self.dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn read(&self, path: &str) -> String {
            fs::read_to_string(self.dir.join(path)).unwrap()
        }

        fn exists(&self, path: &str) -> bool {
            fs::symlink_metadata(self.dir.join(path)).is_ok()
        }

        fn start(&self, mode: TransferMode, sources: &[&str], folder: &str) {
            let sources: Vec<_> = sources.iter().map(|source| self.dir.join(source).display().to_string()).collect();
            let folder = self.dir.join(folder).display().to_string();
            self.transfers.start(self.client, 1, mode, &sources, &folder).unwrap();
        }

        /// The job's events up to the first in `state`, which is returned.
        fn until(&self, state: TransferState) -> Transfer {
            loop {
                let transfer: Transfer =
                    serde_json::from_value(next_named(&self.events, "files_transfer", WAIT)).unwrap();
                if transfer.state == state {
                    return transfer;
                }
                assert!(
                    !matches!(transfer.state, TransferState::Done | TransferState::Cancelled | TransferState::Failed),
                    "ended {:?} ({:?}) waiting for {state:?}",
                    transfer.state,
                    transfer.error
                );
            }
        }

        fn answer(&self, clash: &Clash, answer: ClashAnswer, all: bool) {
            self.transfers.answer(self.client, 1, clash.id, answer, all).unwrap();
        }

        fn names(&self, folder: &str) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(self.dir.join(folder))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    #[test]
    fn a_drop_on_the_same_drive_moves_and_on_another_copies() {
        let fixture = Fixture::new("same-drive");
        fixture.write("from/Photos/a.png", "1");
        let path = |name: &str| fixture.dir.join(name).display().to_string();
        let sources = [path("from/Photos"), path("from/Photos/a.png")];
        assert_eq!(same_drive(&sources, &path("to")), Ok(true));
        assert_eq!(same_drive(&sources, "/proc"), Ok(false), "/proc is a file system of its own");
        assert_eq!(
            same_drive(&[path("from/Photos"), "/proc/self".into()], &path("to")),
            Ok(false),
            "one away is enough"
        );
        assert!(same_drive(&[path("from/gone.txt")], &path("to")).unwrap_err().contains("doesn't exist"));
        assert!(same_drive(&sources, "relative").is_err(), "paths are checked");
    }

    #[test]
    fn a_copy_brings_files_folders_and_links_with_their_times() {
        let fixture = Fixture::new("copy");
        fixture.write("from/Photos/2025/a.png", "12345");
        fixture.write("from/notes.txt", "hi");
        std::os::unix::fs::symlink("2025/a.png", fixture.dir.join("from/Photos/latest")).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options().write(true).open(fixture.dir.join("from/notes.txt")).unwrap().set_modified(old).unwrap();
        fixture.start(TransferMode::Copy, &["from/Photos", "from/notes.txt"], "to");
        let counting = fixture.until(TransferState::Counting);
        assert_eq!((counting.items, counting.name.as_str()), (2, "Photos"));
        let done = fixture.until(TransferState::Done);
        assert_eq!((done.done_bytes, done.total_bytes), (7, 7));
        assert_eq!(fixture.read("to/Photos/2025/a.png"), "12345");
        assert_eq!(fs::read_link(fixture.dir.join("to/Photos/latest")).unwrap(), Path::new("2025/a.png"));
        assert_eq!(fs::metadata(fixture.dir.join("to/notes.txt")).unwrap().modified().unwrap(), old);
        assert!(fixture.exists("from/notes.txt"), "a copy leaves the original");
        assert_eq!(fixture.names("to/Photos"), ["2025", "latest"], "no partial files left");
    }

    #[test]
    fn a_copy_into_its_own_folder_gets_a_number_and_a_move_there_stays() {
        let fixture = Fixture::new("same-folder");
        fixture.write("from/cover.png", "png");
        fixture.start(TransferMode::Copy, &["from/cover.png"], "from");
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("from"), ["cover (2).png", "cover.png"]);
        fixture.start(TransferMode::Move, &["from/cover.png"], "from");
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("from"), ["cover (2).png", "cover.png"]);
    }

    #[test]
    fn a_taken_file_name_asks_and_replace_for_all_answers_the_rest() {
        let fixture = Fixture::new("replace");
        for name in ["a.txt", "b.txt", "c.txt"] {
            fixture.write(&format!("from/{name}"), "new");
            fixture.write(&format!("to/{name}"), "old!");
        }
        fixture.start(TransferMode::Copy, &["from/a.txt", "from/b.txt", "from/c.txt"], "to");
        let asking = fixture.until(TransferState::Asking);
        let clash = asking.clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.folder, clash.others), ("a.txt", false, 2));
        assert_eq!((clash.original.size, clash.replacement.size), (Some(4), Some(3)));
        fixture.answer(&clash, ClashAnswer::Skip, false);
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.others), ("b.txt", 1));
        assert!(
            fixture.transfers.answer(fixture.client, 1, clash.id, ClashAnswer::Merge, false).is_err(),
            "files don't merge"
        );
        fixture.answer(&clash, ClashAnswer::Replace, true);
        let done = fixture.until(TransferState::Done);
        assert_eq!(done.total_bytes, 6, "the skipped file comes off the total");
        assert_eq!(
            [fixture.read("to/a.txt"), fixture.read("to/b.txt"), fixture.read("to/c.txt")],
            ["old!", "new", "new"]
        );
    }

    #[test]
    fn a_file_on_a_folder_is_always_asked_about_and_replaces_it_whole() {
        let fixture = Fixture::new("mixed");
        fixture.write("from/a.txt", "new");
        fixture.write("to/a.txt", "old");
        fixture.write("from/Photos", "a file now");
        fixture.write("to/Photos/kept.png", "png");
        fixture.start(TransferMode::Copy, &["from/a.txt", "from/Photos"], "to");
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!(clash.others, 0, "a file on a folder isn't one of the file clashes");
        fixture.answer(&clash, ClashAnswer::Replace, true);
        let mixed = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((mixed.name.as_str(), mixed.folder, mixed.original.items), ("Photos", false, Some(1)));
        fixture.answer(&mixed, ClashAnswer::Replace, false);
        fixture.until(TransferState::Done);
        assert_eq!(fixture.read("to/Photos"), "a file now");
        assert_eq!(fixture.names("to"), ["Photos", "a.txt"], "nothing set aside is left");
    }

    #[test]
    fn a_folder_on_a_folder_merges_and_asks_about_the_files_inside() {
        let fixture = Fixture::new("merge");
        fixture.write("from/Photos/new.png", "new");
        fixture.write("from/Photos/same.png", "theirs");
        fixture.write("to/Photos/old.png", "old");
        fixture.write("to/Photos/same.png", "mine");
        fixture.start(TransferMode::Move, &["from/Photos"], "to");
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.folder), ("Photos", true));
        assert_eq!((clash.original.items, clash.replacement.items), (Some(2), Some(2)));
        assert!(fixture.transfers.answer(fixture.client, 1, clash.id, ClashAnswer::Replace, false).is_err());
        fixture.answer(&clash, ClashAnswer::Merge, false);
        let inner = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((inner.name.as_str(), inner.folder), ("same.png", false));
        fixture.answer(&inner, ClashAnswer::Skip, false);
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("to/Photos"), ["new.png", "old.png", "same.png"]);
        assert_eq!(fixture.read("to/Photos/same.png"), "mine");
        assert_eq!(fixture.names("from/Photos"), ["same.png"], "what was skipped stays where it was");
    }

    #[test]
    fn cancelling_while_asking_ends_the_job_and_keeps_what_was_there() {
        let fixture = Fixture::new("cancel");
        fixture.write("from/a.txt", "new");
        fixture.write("to/a.txt", "old");
        fixture.start(TransferMode::Copy, &["from/a.txt"], "to");
        fixture.until(TransferState::Asking);
        fixture.transfers.cancel(fixture.client, 1);
        fixture.until(TransferState::Cancelled);
        assert_eq!(fixture.read("to/a.txt"), "old");
        assert_eq!(fixture.names("to"), ["a.txt"]);
    }

    #[test]
    fn a_folder_cant_go_into_itself() {
        let fixture = Fixture::new("itself");
        fixture.write("from/Photos/a.png", "png");
        fixture.start(TransferMode::Copy, &["from/Photos"], "from/Photos");
        let failed = fixture.until(TransferState::Failed);
        assert!(failed.error.unwrap().contains("into itself"));
        assert_eq!(fixture.names("from/Photos"), ["a.png"]);
    }

    #[test]
    fn a_job_number_runs_once_and_requests_are_checked() {
        let fixture = Fixture::new("checks");
        fixture.write("from/a.txt", "a");
        fixture.write("to/a.txt", "b");
        fixture.start(TransferMode::Copy, &["from/a.txt"], "to");
        fixture.until(TransferState::Asking);
        let again = fixture.transfers.start(fixture.client, 1, TransferMode::Copy, &["/tmp".into()], "/tmp");
        assert!(again.unwrap_err().contains("already running"));
        assert!(fixture.transfers.start(fixture.client, 2, TransferMode::Copy, &[], "/tmp").is_err());
        assert!(fixture.transfers.start(fixture.client, 2, TransferMode::Copy, &["relative".into()], "/tmp").is_err());
        fixture.transfers.client_gone(fixture.client);
        assert!(
            fixture.transfers.answer(fixture.client, 1, 1, ClashAnswer::Skip, false).is_err(),
            "gone with its client"
        );
    }

    #[test]
    fn extract_all_makes_a_folder_named_after_the_archive_and_merges_into_one_there() {
        let fixture = Fixture::new("extract");
        crate::files::archive::tests::write_zip(
            &fixture.dir.join("from/photos.zip"),
            &[("a.png", "new"), ("2025/b.png", "bbb")],
        );
        fixture.write("to/photos/a.png", "old");
        fixture.start(TransferMode::Extract, &["from/photos.zip"], "to");
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.folder), ("photos", true));
        fixture.answer(&clash, ClashAnswer::Merge, false);
        let inner = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!(inner.name, "a.png");
        fixture.answer(&inner, ClashAnswer::Replace, false);
        let done = fixture.until(TransferState::Done);
        assert_eq!((done.done_bytes, done.total_bytes), (6, 6), "measured by what was extracted");
        assert_eq!(fixture.read("to/photos/a.png"), "new");
        assert_eq!(fixture.read("to/photos/2025/b.png"), "bbb");
        assert_eq!(fixture.names("to"), ["photos"], "nothing extracted is left aside");
        assert!(fixture.exists("from/photos.zip"), "the archive stays");
    }

    #[test]
    fn extract_here_puts_the_contents_in_the_folder() {
        let fixture = Fixture::new("extract-here");
        crate::files::archive::tests::write_zip(
            &fixture.dir.join("from/docs.zip"),
            &[("a.txt", "a"), ("b/c.txt", "c")],
        );
        fixture.start(TransferMode::ExtractHere, &["from/docs.zip"], "from");
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("from"), ["a.txt", "b", "docs.zip"]);
        assert_eq!(fixture.read("from/b/c.txt"), "c");
    }

    #[test]
    fn a_file_that_isnt_an_archive_fails_to_extract_and_leaves_nothing() {
        let fixture = Fixture::new("extract-not");
        fixture.write("from/fake.zip", "not a zip");
        fixture.write("from/notes.txt", "text");
        fixture.start(TransferMode::Extract, &["from/fake.zip"], "to");
        assert!(fixture.until(TransferState::Failed).error.unwrap().contains("fake.zip"));
        fixture
            .transfers
            .start(
                fixture.client,
                2,
                TransferMode::Extract,
                &[fixture.dir.join("from/notes.txt").display().to_string()],
                &fixture.dir.join("to").display().to_string(),
            )
            .unwrap();
        assert!(fixture.until(TransferState::Failed).error.unwrap().contains("isn't an archive"));
        assert!(fixture.names("to").is_empty(), "no hidden folder left behind");
    }

    #[test]
    fn compress_names_the_zip_after_the_first_item_and_numbers_a_taken_name() {
        let fixture = Fixture::new("compress");
        fixture.write("from/report.pdf", "pdf");
        fixture.write("from/Photos/a.png", "png");
        fixture.write("from/report.zip", "taken");
        fixture.start(TransferMode::Compress, &["from/report.pdf", "from/Photos"], "from");
        let done = fixture.until(TransferState::Done);
        assert_eq!((done.done_bytes, done.total_bytes), (6, 6));
        assert_eq!(fixture.names("from"), ["Photos", "report (2).zip", "report.pdf", "report.zip"]);
        let mode = fs::metadata(fixture.dir.join("from/report (2).zip")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        fs::set_permissions(fixture.dir.join("from/report.pdf"), fs::Permissions::from_mode(0o600)).unwrap();
        fixture
            .transfers
            .start(
                fixture.client,
                2,
                TransferMode::Compress,
                &[fixture.dir.join("from/report.pdf").display().to_string()],
                &fixture.dir.join("from").display().to_string(),
            )
            .unwrap();
        fixture.until(TransferState::Done);
        let private = fs::metadata(fixture.dir.join("from/report (3).zip")).unwrap().permissions().mode() & 0o777;
        assert_eq!(private, 0o600, "a zip of a private file stays private");
        let inside = crate::files::archive::Inside::of(&fixture.dir.join("from/report (2).zip/Photos")).unwrap();
        let copy = crate::files::archive::open_copy(
            &crate::files::archive::Inside { inner: "Photos/a.png".into(), ..inside },
            &fixture.dir.join("opened"),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(copy).unwrap(), "png");
    }

    #[test]
    fn a_skipped_extracted_file_still_counts_and_a_zip_cant_go_into_what_it_packs() {
        let fixture = Fixture::new("extract-skip");
        crate::files::archive::tests::write_zip(&fixture.dir.join("from/docs.zip"), &[("a.txt", "new")]);
        fixture.write("to/a.txt", "old");
        fixture.start(TransferMode::ExtractHere, &["from/docs.zip"], "to");
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        fixture.answer(&clash, ClashAnswer::Skip, false);
        let done = fixture.until(TransferState::Done);
        assert_eq!((done.done_bytes, done.total_bytes), (3, 3));
        assert_eq!(fixture.names("to"), ["a.txt"]);
        fixture.write("from/Photos/2025/a.png", "png");
        fixture.start(TransferMode::Compress, &["from/Photos"], "from/Photos/2025");
        assert!(fixture.until(TransferState::Failed).error.unwrap().contains("into itself"));
        assert_eq!(fixture.names("from/Photos/2025"), ["a.png"]);
    }
}
