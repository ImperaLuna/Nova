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
//! it's being made gets the same answer. Each client waits for at most
//! [`FILES_THUMBNAILS_MAX_WAITING`]; past that its longest waiting is
//! answered with none.
//!
//! The thumbnailers are read as the module starts and again whenever the
//! MIME database is (a package came or went), so one installed later is
//! used without a restart.

mod cache;
mod thumbnailers;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::mime::{Mime, MimeDb};
use cache::{Cache, Original};
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

#[derive(Default)]
struct Queues {
    /// To look up in the cache, the latest last.
    asked: VecDeque<Ask>,
    /// To make, the latest asked for last.
    to_make: VecDeque<Make>,
    /// The ask being looked up, while it's still wanted.
    looking: Option<Ask>,
    /// Files being made, and who else asked for them meanwhile.
    making: HashMap<PathBuf, Vec<Ask>>,
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
        for waiting in self.making.values_mut() {
            waiting.retain(|ask| ask.client != client || !matches(&ask.path));
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

    /// Queues `make` to be made, in the order it was asked for.
    fn queue_to_make(&mut self, make: Make) {
        let at = self.to_make.partition_point(|queued| queued.ask.number < make.ask.number);
        self.to_make.insert(at, make);
    }
}

/// The thumbnailers, as read with one MIME database.
#[derive(Default)]
struct Known {
    db: Option<Arc<MimeDb>>,
    thumbnailers: Vec<Thumbnailer>,
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
            self.shared.answer(&dropped, None);
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
                    self.answer(&ask, found);
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
        while let Some(make) = self.next(|queues| {
            let at = queues.to_make.iter().rposition(|make| !queues.making.contains_key(&make.ask.path));
            match at.and_then(|at| queues.to_make.remove(at)) {
                Some(make) => {
                    queues.making.insert(make.ask.path.clone(), Vec::new());
                    Some(make)
                }
                // Each one waiting is being made: it's answered with that.
                None => {
                    while let Some(make) = queues.to_make.pop_back() {
                        if let Some(waiting) = queues.making.get_mut(&make.ask.path) {
                            waiting.push(make.ask);
                        }
                    }
                    None
                }
            }
        }) {
            let made = self.made(&make, worker);
            let others = lock(&self.queues).making.remove(&make.ask.path).unwrap_or_default();
            for ask in std::iter::once(&make.ask).chain(&others) {
                self.answer(ask, made.clone());
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

    /// `ask`'s thumbnail from the cache, or none when it can't have one;
    /// else what to make.
    fn looked_up(&self, ask: &Ask) -> Result<Option<PathBuf>, Box<Make>> {
        let Some(cache) = &self.cache else { return Ok(None) };
        let Ok(metadata) = std::fs::metadata(&ask.path) else { return Ok(None) };
        if !metadata.is_file() || cache.holds(&ask.path) {
            return Ok(None);
        }
        let original = Original::new(&ask.path, &metadata);
        if let Some(found) = cache.found(&original) {
            return Ok(Some(found));
        }
        if cache.failed(&original) {
            return Ok(None);
        }
        let Some(name) = ask.path.file_name().and_then(|name| name.to_str()) else { return Ok(None) };
        let db = self.mime.current();
        let mime = db.type_by_name(name, &metadata).to_string();
        match self.thumbnailer(&db, &mime) {
            Some(thumbnailer) => Err(Box::new(Make { ask: ask.clone(), original, thumbnailer })),
            None => Ok(None),
        }
    }

    /// The thumbnailer for type `mime`, reading them again when the MIME
    /// database was.
    fn thumbnailer(&self, db: &Arc<MimeDb>, mime: &str) -> Option<Thumbnailer> {
        let mut known = lock(&self.known);
        if !known.db.as_ref().is_some_and(|seen| Arc::ptr_eq(seen, db)) {
            let (found, problems) = thumbnailers::load(&self.sources.data_dirs, &self.sources.path_var);
            for problem in problems {
                self.logger.warn(LOG_SOURCE, &format!("thumbnailer {problem}"));
            }
            let names: Vec<_> = found.iter().map(|thumbnailer| thumbnailer.name.as_str()).collect();
            self.logger.info(
                LOG_SOURCE,
                &format!("thumbnailers: {}", if names.is_empty() { "none".into() } else { names.join(", ") }),
            );
            *known = Known { db: Some(Arc::clone(db)), thumbnailers: found };
        }
        thumbnailers::for_type(&known.thumbnailers, mime, db).cloned()
    }

    /// Runs `make`'s thumbnailer as maker `worker` and stores what it made;
    /// none when it failed, which is marked so it isn't tried again.
    fn made(&self, make: &Make, worker: usize) -> Option<PathBuf> {
        let cache = self.cache.as_ref()?;
        let original = &make.original;
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
            Ok(output) if output.status.success() => cache.store(original, &scratch),
            Ok(output) => Err(format!("{}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
            Err(e) => Err(e.to_string()),
        };
        let _ = std::fs::remove_file(&scratch);
        match stored {
            Ok(stored) => Some(stored),
            Err(e) => {
                let name = &make.thumbnailer.name;
                self.logger.info(LOG_SOURCE, &format!("no thumbnail of {} from {name}: {e}", original.path.display()));
                if let Err(e) = cache.mark_failed(original) {
                    self.logger.warn(LOG_SOURCE, &format!("thumbnails: {e}"));
                }
                None
            }
        }
    }

    /// Tells `ask`'s client about its thumbnail, unless stopped.
    fn answer(&self, ask: &Ask, thumbnail: Option<PathBuf>) {
        if lock(&self.queues).stopped {
            return;
        }
        let thumbnail = thumbnail.map(wire_text);
        self.hub.send_to(ask.client, &Event::FilesThumbnail { path: ask.wire.clone(), thumbnail });
    }
}

#[cfg(test)]
mod tests;
