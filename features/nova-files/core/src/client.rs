use futures_core::Stream;
use nova_base::lock;
use nova_session_bus::{Bus, Stop};
use std::collections::HashMap;
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};
use zbus::message::{Flags, Type};
use zbus::{Connection, MatchRule, Message, MessageStream};

pub const APP_ID: &str = "io.github.ImperaLuna.NovaFiles";
pub const PATH: &str = "/io/github/ImperaLuna/NovaFiles";
pub const INTERFACE: &str = "io.github.ImperaLuna.NovaFiles.Search1";
const SEARCH: &str = "Search";
const COMPLETE: &str = "Complete";
const CANCEL: &str = "Cancel";
const OPEN: &str = "Open";
const SHOW_IN_FOLDER: &str = "ShowInFolder";
const FOUND: &str = "Found";
const FILE_MANAGER: &str = "org.freedesktop.FileManager1";
const FILE_MANAGER_PATH: &str = "/org/freedesktop/FileManager1";
const SHOW_FOLDERS: &str = "ShowFolders";
const SHOW_ITEMS: &str = "ShowItems";
const URI_PATH_BYTES: &[u8] = b"/-._~!$&'()*+,;=:@";
const FAILED: &str = "Failed";
const QUEUED_SIGNALS: usize = 64;

#[derive(Debug, PartialEq)]
enum Stage {
    Found { paths: Vec<PathBuf>, done: bool },
    Failed(String),
}

type Waiting = Arc<Mutex<HashMap<u64, Sender<Stage>>>>;

pub struct Client {
    connection: Connection,
    pub(crate) first_stage_wait: Duration,
    waiting: Waiting,
    going: Mutex<Option<u64>>,
    next_id: AtomicU64,
    stop: Arc<Stop>,
}

impl Client {
    pub fn connect(bus: &Bus, timeout: Duration) -> Result<Self, String> {
        let connection = zbus::block_on(bus.builder().method_timeout(timeout).build()).map_err(|e| e.to_string())?;
        let rule = MatchRule::builder().msg_type(Type::Signal).interface(INTERFACE).path(PATH).build();
        let rule = rule.map_err(|e| e.to_string())?;
        let size = std::num::NonZeroUsize::new(QUEUED_SIGNALS);
        let signals =
            zbus::block_on(MessageStream::for_match_rule(rule, &connection, size)).map_err(|e| e.to_string())?;
        let waiting = Waiting::default();
        let stop = Arc::new(Stop::default());
        let (reader_waiting, reader_stop) = (Arc::clone(&waiting), Arc::clone(&stop));
        std::thread::spawn(move || zbus::block_on(read(signals, &reader_waiting, &reader_stop)));
        Ok(Self {
            connection,
            first_stage_wait: timeout,
            waiting,
            going: Mutex::new(None),
            next_id: AtomicU64::new(1),
            stop,
        })
    }

    pub fn search(&self, query: &str, limit: u32, wait: Duration) -> Result<Vec<PathBuf>, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, stages) = mpsc::channel();
        lock(&self.waiting).insert(id, sender);
        let found = self.run_search((id, query, limit), wait, &stages);
        lock(&self.waiting).remove(&id);
        found
    }

    fn run_search(
        &self,
        call: (u64, &str, u32),
        wait: Duration,
        stages: &mpsc::Receiver<Stage>,
    ) -> Result<Vec<PathBuf>, String> {
        let id = call.0;
        let older = {
            let mut going = lock(&self.going);
            if going.is_some_and(|newer| newer > id) {
                return Ok(Vec::new());
            }
            going.replace(id)
        };
        if let Some(older) = older {
            lock(&self.waiting).remove(&older);
            self.cancel(older);
        }
        zbus::block_on(self.connection.call_method(Some(APP_ID), PATH, Some(INTERFACE), SEARCH, &call))
            .map_err(|e| e.to_string())?;
        let mut deadline = Instant::now() + self.first_stage_wait;
        let mut paths = Vec::new();
        let mut heard = false;
        loop {
            match stages.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Stage::Found { paths: found, done }) => {
                    paths.extend(found);
                    if done {
                        self.finished(id);
                        return Ok(paths);
                    }
                    if !heard {
                        heard = true;
                        deadline = Instant::now() + wait;
                    }
                }
                Ok(Stage::Failed(reason)) => return Err(format!("Nova Files couldn't search: {reason}")),
                Err(RecvTimeoutError::Timeout) if heard => return Ok(paths),
                Err(RecvTimeoutError::Timeout) => return Err("Nova Files didn't answer in time".into()),
                Err(RecvTimeoutError::Disconnected) => return Ok(paths),
            }
        }
    }

    fn finished(&self, id: u64) {
        let mut going = lock(&self.going);
        if *going == Some(id) {
            *going = None;
        }
    }

    pub fn complete(&self, typed: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        let reply =
            zbus::block_on(self.connection.call_method(Some(APP_ID), PATH, Some(INTERFACE), COMPLETE, &(typed, limit)))
                .map_err(|e| e.to_string())?;
        let entries: Vec<(String, bool)> = reply.body().deserialize().map_err(|e| e.to_string())?;
        Ok(entries.into_iter().map(|(path, _folder)| PathBuf::from(path)).collect())
    }

    pub fn open(&self, path: &Path) -> Result<(), String> {
        self.call_with_path(OPEN, path)
    }

    pub fn show_in_folder(&self, path: &Path) -> Result<(), String> {
        self.call_with_path(SHOW_IN_FOLDER, path)
    }

    pub fn show_folder_elsewhere(&self, path: &Path) -> Result<(), String> {
        self.call_file_manager(SHOW_FOLDERS, path)
    }

    pub fn show_item_elsewhere(&self, path: &Path) -> Result<(), String> {
        self.call_file_manager(SHOW_ITEMS, path)
    }

    fn call_file_manager(&self, method: &str, path: &Path) -> Result<(), String> {
        let body = (vec![file_uri(path)], "");
        let call =
            self.connection.call_method(Some(FILE_MANAGER), FILE_MANAGER_PATH, Some(FILE_MANAGER), method, &body);
        zbus::block_on(call).map(drop).map_err(|e| format!("no file manager answered: {e}"))
    }

    fn call_with_path(&self, method: &str, path: &Path) -> Result<(), String> {
        let path =
            path.to_str().ok_or_else(|| format!("{} isn't UTF-8, so Nova Files can't take it", path.display()))?;
        let platform_data: HashMap<&str, zbus::Variant<'_>> = HashMap::new();
        zbus::block_on(self.connection.call_method(Some(APP_ID), PATH, Some(INTERFACE), method, &(path, platform_data)))
            .map(drop)
            .map_err(|e| e.to_string())
    }

    fn cancel(&self, id: u64) {
        let message = Message::method_call(PATH, CANCEL)
            .destination(APP_ID)
            .interface(INTERFACE)
            .with_flags(Flags::NO_REPLY_EXPECTED)
            .build(&(id,));
        if let Ok(message) = message {
            let _ = zbus::block_on(self.connection.send(&message));
        }
    }

    pub fn stop(&self) {
        self.stop.stop();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

pub fn file_uri(path: &Path) -> String {
    use std::fmt::Write;
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || URI_PATH_BYTES.contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

async fn read(signals: MessageStream, waiting: &Waiting, stop: &Stop) {
    let mut signals = pin!(signals);
    loop {
        let next = poll_fn(|cx| match stop.poll(cx) {
            Poll::Ready(()) => Poll::Ready(None),
            Poll::Pending => signals.as_mut().poll_next(cx),
        })
        .await;
        let message = match next {
            None => return,
            Some(Err(_)) => continue,
            Some(Ok(message)) => message,
        };
        if let Some((id, stage)) = stage(&message)
            && let Some(sender) = lock(waiting).get(&id)
        {
            let _ = sender.send(stage);
        }
    }
}

fn stage(message: &Message) -> Option<(u64, Stage)> {
    let header = message.header();
    let body = message.body();
    match header.member()?.as_str() {
        FOUND => {
            let (id, hits, done): (u64, Vec<(String, String)>, bool) = body.deserialize().ok()?;
            let paths = hits.into_iter().map(|(path, _matched)| PathBuf::from(path)).collect();
            Some((id, Stage::Found { paths, done }))
        }
        FAILED => {
            let (id, reason): (u64, String) = body.deserialize().ok()?;
            Some((id, Stage::Failed(reason)))
        }
        _ => None,
    }
}
