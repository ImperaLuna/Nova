//! Thumbnails for Nova Files' Icons view (`files_thumbnail`): pictures of
//! images and videos, kept in the shared freedesktop cache (`cache.rs`) and
//! made by the system's thumbnailers (`thumbnailers.rs`).
//!
//! A window asks for the files it shows and cancels those that scroll
//! away, so only what's on screen is worked on. Asks wait in two queues,
//! the latest first: one thread looks each up in the cache (a stat and a
//! PNG header, quick), answering with what's there, with none for a file
//! no thumbnailer takes, or passing it on; [`MAKERS`] threads run the
//! thumbnailers for the rest, one file at a time each, so a big video
//! never holds up a thumbnail that's already cached. A file asked for while
//! it's being made, as it is now, gets the same answer; one that changed
//! meanwhile is made again after. Each answer says which version of the
//! file it's for. Each client waits for at most
//! [`FILES_THUMBNAILS_MAX_WAITING`]; past that its longest waiting is
//! answered with none.
//!
//! A file is marked failed in the cache only when its thumbnailer ran and
//! failed on it; a thumbnailer that can't start, runs past
//! [`MAKE_TIMEOUT`] or a cache that can't be written is only logged, so the
//! file is tried again next time.
//!
//! The thumbnailers are read when first needed and again whenever the MIME
//! database is or a thumbnailers folder changed (a package came or went),
//! so one installed later is used without a restart.

mod cache;
mod thumbnailers;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::mime::{Mime, MimeDb};
use cache::{Cache, Failed, Original};
use nova_protocol::{Event, FILES_THUMBNAIL_PIXELS, FILES_THUMBNAILS_MAX_WAITING, wire_text};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use thumbnailers::{Job, Thumbnailer};

const LOG_SOURCE: &str = "files";
/// Threads running thumbnailers: two keep a folder of photos coming
/// without taking the machine.
const MAKERS: usize = 2;
/// How long a thumbnailer may take for one file before it's stopped and
/// the file marked failed.
const MAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// What a thumbnailer gets of novad's environment: where things are and
/// the language, and the variables image and video libraries find their
/// plugins by (NixOS sets them for the session).
const PASSED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_DATA_DIRS",
    "XDG_RUNTIME_DIR",
    "GDK_PIXBUF_MODULE_FILE",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_PATH_1_0",
    "GIO_EXTRA_MODULES",
];

/// Where thumbnails come from.
#[derive(Clone)]
pub struct Sources {
    /// The shared cache; none without a home, when there are no thumbnails.
    pub cache: Option<PathBuf>,
    /// The data folders thumbnailers are read from.
    pub data_dirs: Vec<PathBuf>,
    /// Where their programs are looked for.
    pub path_var: String,
}

impl Sources {
    /// The user's cache and the session's data folders.
    pub fn system(data_dirs: Vec<PathBuf>, path_var: &str) -> Self {
        let cache = nova_paths::cache_home().map(|cache| cache.join("thumbnails"));
        Self { cache, data_dirs, path_var: path_var.to_string() }
    }

    /// A cache inside `dir` and no thumbnailers, for tests.
    #[cfg(test)]
    pub fn under(dir: &Path) -> Self {
        Self { cache: Some(dir.join("thumbnails")), data_dirs: vec![], path_var: String::new() }
    }
}

pub struct Thumbnails {
    shared: Arc<Shared>,
}

struct Shared {
    sources: Sources,
    cache: Option<Cache>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    mime: Arc<Mime>,
    queues: Mutex<Queues>,
    /// Woken when something is queued, or on stopping.
    queued: Condvar,
    known: Mutex<Known>,
}

/// One file a client asked for.
#[derive(Clone)]
struct Ask {
    client: ClientId,
    /// As the client named it, for the answer.
    wire: String,
    path: PathBuf,
    /// When it was asked for: asks are numbered in order.
    number: u64,
}

/// One file to run a thumbnailer on.
struct Make {
    ask: Ask,
    original: Original,
    thumbnailer: Thumbnailer,
}

/// What an ask is answered with.
#[derive(Clone, Default)]
struct Answer {
    thumbnail: Option<PathBuf>,
    /// When the file it's for last changed; none when it's gone.
    modified_ms: Option<i64>,
}

/// A file being made.
struct Making {
    /// The version being made.
    mtime: i64,
    /// Who it's for, the ask that started it first.
    waiting: Vec<Ask>,
}

#[derive(Default)]
struct Queues {
    /// To look up in the cache, the latest last.
    asked: VecDeque<Ask>,
    /// To make, the latest asked for last.
    to_make: VecDeque<Make>,
    /// The ask being looked up, while it's still wanted.
    looking: Option<Ask>,
    /// Files being made.
    making: HashMap<PathBuf, Making>,
    /// The next ask's number.
    next_number: u64,
    stopped: bool,
}

impl Queues {
    /// Forgets client `client`'s waiting asks that `matches`.
    fn remove(&mut self, client: ClientId, matches: impl Fn(&Path) -> bool) {
        self.asked.retain(|ask| ask.client != client || !matches(&ask.path));
        self.to_make.retain(|make| make.ask.client != client || !matches(&make.ask.path));
        if self.looking.as_ref().is_some_and(|ask| ask.client == client && matches(&ask.path)) {
            self.looking = None;
        }
        for making in self.making.values_mut() {
            making.waiting.retain(|ask| ask.client != client || !matches(&ask.path));
        }
    }

    /// The longest waiting of `client`'s asks, taken out, once it has more
    /// than it may.
    fn over_limit(&mut self, client: ClientId) -> Option<Ask> {
        let to_make = self.to_make.iter().map(|make| &make.ask);
        let waiting = self.asked.iter().chain(to_make).chain(&self.looking).filter(|ask| ask.client == client);
        if waiting.clone().count() <= FILES_THUMBNAILS_MAX_WAITING {
            return None;
        }
        let oldest = waiting.map(|ask| ask.number).min()?;
        if let Some(at) = self.asked.iter().position(|ask| ask.number == oldest) {
            return self.asked.remove(at);
        }
        if let Some(at) = self.to_make.iter().position(|make| make.ask.number == oldest) {
            return self.to_make.remove(at).map(|make| make.ask);
        }
        self.looking.take()
    }

    /// Queues `make` to be made, in the order it was asked for, or has it
    /// wait for the same version of the file being made already.
    fn queue_to_make(&mut self, make: Make) {
        if let Some(making) = self.making.get_mut(&make.ask.path)
            && making.mtime == make.original.mtime
        {
            making.waiting.push(make.ask);
            return;
        }
        let at = self.to_make.partition_point(|queued| queued.ask.number < make.ask.number);
        self.to_make.insert(at, make);
    }

    /// The latest queued file that isn't being made already, marked as
    /// being made.
    fn take_to_make(&mut self) -> Option<Make> {
        let at = self.to_make.iter().rposition(|make| !self.making.contains_key(&make.ask.path))?;
        let make = self.to_make.remove(at)?;
        let making = Making { mtime: make.original.mtime, waiting: vec![make.ask.clone()] };
        self.making.insert(make.ask.path.clone(), making);
        Some(make)
    }
}

/// The thumbnailers, as read with one MIME database and their folders as
/// they were.
#[derive(Default)]
struct Known {
    db: Option<Arc<MimeDb>>,
    folders: Vec<Folder>,
    thumbnailers: Vec<Thumbnailer>,
}

/// A thumbnailers folder as it was: where it really is (a NixOS profile
/// links somewhere new each generation) and when it last changed; none when
/// it isn't there.
type Folder = Option<(PathBuf, Option<std::time::SystemTime>)>;

/// The thumbnailers folders in `data_dirs`, as they are now: a few stats,
/// cheap beside running a thumbnailer.
fn folders(data_dirs: &[PathBuf]) -> Vec<Folder> {
    data_dirs
        .iter()
        .map(|dir| {
            let real = std::fs::canonicalize(dir.join("thumbnailers")).ok()?;
            let changed = std::fs::metadata(&real).and_then(|metadata| metadata.modified()).ok();
            Some((real, changed))
        })
        .collect()
}

impl Thumbnails {
    /// Starts the threads, looking up types in `mime`.
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>, mime: Arc<Mime>) -> Self {
        let cache = sources.cache.clone().map(Cache::new);
        let shared = Arc::new(Shared {
            sources,
            cache,
            hub,
            logger,
            mime,
            queues: Mutex::default(),
            queued: Condvar::new(),
            known: Mutex::default(),
        });
        let lookup = Arc::clone(&shared);
        std::thread::spawn(move || lookup.look_up());
        for worker in 0..MAKERS {
            let maker = Arc::clone(&shared);
            std::thread::spawn(move || maker.make(worker));
        }
        Self { shared }
    }

    /// Client `client` wants a thumbnail of the file at `path` (wire text).
    pub fn request(&self, client: ClientId, path: &str) -> Result<(), String> {
        let checked = super::views::checked_path(path)?;
        let dropped = {
            let mut queues = lock(&self.shared.queues);
            if queues.stopped {
                return Err("Nova Files is stopping".into());
            }
            // Asked again: it moves to the front.
            queues.remove(client, |waiting| waiting == checked);
            let number = queues.next_number;
            queues.next_number += 1;
            queues.asked.push_back(Ask { client, wire: path.to_string(), path: checked, number });
            queues.over_limit(client)
        };
        self.shared.queued.notify_all();
        if let Some(dropped) = dropped {
            self.shared.answer(&dropped, &Answer::default());
        }
        Ok(())
    }

    /// Client `client` no longer shows the file at `path` (wire text).
    pub fn cancel(&self, client: ClientId, path: &str) {
        if let Ok(checked) = super::views::checked_path(path) {
            lock(&self.shared.queues).remove(client, |waiting| waiting == checked);
        }
    }

    /// Client `client` hung up: nothing it asked for is made.
    pub fn client_gone(&self, client: ClientId) {
        lock(&self.shared.queues).remove(client, |_| true);
    }

    /// Stops the threads; thumbnailers running finish, but nothing more is
    /// sent.
    pub fn stop(&self) {
        let mut queues = lock(&self.shared.queues);
        *queues = Queues { stopped: true, ..Queues::default() };
        drop(queues);
        self.shared.queued.notify_all();
    }
}

impl Shared {
    /// The lookup thread: the cache, then the queue to make.
    fn look_up(&self) {
        while let Some(ask) = self.next(|queues| {
            queues.looking = queues.asked.pop_back();
            queues.looking.clone()
        }) {
            let looked_up = self.looked_up(&ask);
            let mut queues = lock(&self.queues);
            // Cancelled meanwhile: it scrolled away.
            if queues.looking.take().is_none() {
                continue;
            }
            match looked_up {
                Ok(found) => {
                    drop(queues);
                    self.answer(&ask, &found);
                }
                Err(make) => {
                    queues.queue_to_make(*make);
                    drop(queues);
                    self.queued.notify_all();
                }
            }
        }
    }

    /// A maker thread, `worker`.
    fn make(&self, worker: usize) {
        while let Some(make) = self.next(Queues::take_to_make) {
            let answer = Answer { thumbnail: self.made(&make, worker), modified_ms: make.original.modified_ms };
            let waiting = lock(&self.queues).making.remove(&make.ask.path).map(|making| making.waiting);
            // A newer version of it may wait for this one to be done.
            self.queued.notify_all();
            for ask in waiting.iter().flatten() {
                self.answer(ask, &answer);
            }
        }
    }

    /// The next thing `take` finds in the queues, waiting until there is
    /// one; none once stopped.
    fn next<T>(&self, mut take: impl FnMut(&mut Queues) -> Option<T>) -> Option<T> {
        let mut queues = lock(&self.queues);
        loop {
            if queues.stopped {
                return None;
            }
            if let Some(found) = take(&mut queues) {
                return Some(found);
            }
            queues = self.queued.wait(queues).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// `ask`'s answer from the cache, or none when it can't have a
    /// thumbnail; else what to make.
    fn looked_up(&self, ask: &Ask) -> Result<Answer, Box<Make>> {
        let Ok(metadata) = std::fs::metadata(&ask.path) else { return Ok(Answer::default()) };
        let original = Original::new(&ask.path, &metadata);
        let none = Answer { thumbnail: None, modified_ms: original.modified_ms };
        let Some(cache) = &self.cache else { return Ok(none) };
        if !metadata.is_file() || cache.holds(&ask.path) {
            return Ok(none);
        }
        if let Some(found) = cache.found(&original) {
            return Ok(Answer { thumbnail: Some(found), ..none });
        }
        if cache.failed(&original) {
            return Ok(none);
        }
        let db = self.mime.current();
        // From its name, else its first bytes (a camera's `IMG_0001`).
        let Ok(mime) = db.type_of(&ask.path) else { return Ok(none) };
        match self.thumbnailer(&db, &mime) {
            Some(thumbnailer) => Err(Box::new(Make { ask: ask.clone(), original, thumbnailer })),
            None => Ok(none),
        }
    }

    /// The thumbnailer for type `mime`, reading them again when the MIME
    /// database was or a thumbnailers folder changed.
    fn thumbnailer(&self, db: &Arc<MimeDb>, mime: &str) -> Option<Thumbnailer> {
        let folders = folders(&self.sources.data_dirs);
        let mut known = lock(&self.known);
        if !known.db.as_ref().is_some_and(|seen| Arc::ptr_eq(seen, db)) || known.folders != folders {
            let (found, problems) = thumbnailers::load(&self.sources.data_dirs, &self.sources.path_var);
            for problem in problems {
                self.logger.warn(LOG_SOURCE, &format!("thumbnailer {problem}"));
            }
            let names: Vec<_> = found.iter().map(|thumbnailer| thumbnailer.name.as_str()).collect();
            self.logger.info(
                LOG_SOURCE,
                &format!("thumbnailers: {}", if names.is_empty() { "none".into() } else { names.join(", ") }),
            );
            *known = Known { db: Some(Arc::clone(db)), folders, thumbnailers: found };
        }
        thumbnailers::for_type(&known.thumbnailers, mime, db).cloned()
    }

    /// Runs `make`'s thumbnailer as maker `worker` and stores what it made;
    /// none when it failed, which is marked when the file is why.
    fn made(&self, make: &Make, worker: usize) -> Option<PathBuf> {
        let cache = self.cache.as_ref()?;
        let original = &make.original;
        // Made while it waited, for an ask that came in between.
        if let Some(found) = cache.found(original) {
            return Some(found);
        }
        let scratch = match cache.scratch_path(original, worker) {
            Ok(scratch) => scratch,
            Err(e) => {
                self.logger.warn(LOG_SOURCE, &format!("thumbnails: {e}"));
                return None;
            }
        };
        if lock(&self.queues).stopped {
            return None;
        }
        let job = Job { input: &original.path, uri: &original.uri, output: &scratch, size: FILES_THUMBNAIL_PIXELS };
        let (program, args) = make.thumbnailer.command(&job);
        let mut command = Command::new(&program);
        command.args(args).env_clear().envs(PASSED_ENV.iter().filter_map(|name| Some((name, std::env::var_os(name)?))));
        // Not left running if novad goes.
        crate::child::die_with_novad(&mut command, libc::SIGKILL);
        let stored = match crate::child::output_within(command, MAKE_TIMEOUT) {
            // Turned off meanwhile: nothing is stored or marked, as Files
            // turned on again may be making it too.
            Ok(_) if lock(&self.queues).stopped => Err(Failed::Here("Nova Files stopped".into())),
            Ok(output) if output.status.success() => cache.store(original, &scratch),
            Ok(output) => {
                Err(Failed::File(format!("{}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())))
            }
            // Couldn't start, or ran too long on a busy machine: not the
            // file's fault, so it's tried again.
            Err(e) => Err(Failed::Here(e.to_string())),
        };
        let _ = std::fs::remove_file(&scratch);
        let name = &make.thumbnailer.name;
        let failed = |e: &str| format!("no thumbnail of {} from {name}: {e}", original.path.display());
        match stored {
            Ok(stored) => Some(stored),
            Err(Failed::File(e)) => {
                self.logger.info(LOG_SOURCE, &failed(&e));
                if let Err(e) = cache.mark_failed(original) {
                    self.logger.warn(LOG_SOURCE, &format!("thumbnails: {e}"));
                }
                None
            }
            Err(Failed::Here(e)) => {
                self.logger.warn(LOG_SOURCE, &failed(&e));
                None
            }
        }
    }

    /// Tells `ask`'s client `answer`, unless stopped.
    fn answer(&self, ask: &Ask, answer: &Answer) {
        if lock(&self.queues).stopped {
            return;
        }
        let thumbnail = answer.thumbnail.as_ref().map(wire_text);
        let event = Event::FilesThumbnail { path: ask.wire.clone(), modified_ms: answer.modified_ms, thumbnail };
        self.hub.send_to(ask.client, &event);
    }
}

#[cfg(test)]
mod tests;
