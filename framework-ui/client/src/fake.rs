//! A stand-in for novad that replays recorded events, so a feature's UI can
//! run where the real novad can't (a cloud session without the desktop's
//! services) and the client's tests have a real socket to talk to.
//!
//! A recording is what `nova ipc subscribe <topic>` printed on a real
//! desktop, saved as `<topic>.jsonl`: one event per line.
//!
//! ```text
//! $ nova ipc subscribe sound > recorded/sound.jsonl
//! {"event":"sound","data":{"available":true,"outputs":[...]}}
//! {"event":"sound","data":{"available":true,"outputs":[...]}}
//! ```
//!
//! The fake says hello like novad. A client that subscribes to a topic gets
//! the recording's first event at once, as novad sends a topic's state on
//! subscribing, and each later one a [`Recorded::pace`] apart; the last one
//! stays. `ping` is answered; every other command gets an error, since
//! nothing was recorded for it.

use crate::link::read_line;
use nova_carrier::envelope::REQUEST_MAX_BYTES;
use nova_carrier::{Outcome, Response, decode_request, encode};
use nova_daemon_info::{Command, Event};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// The version the fake's hello gives.
pub const VERSION: &str = "fake";
/// The file name ending of a topic's recording.
const RECORDING: &str = "jsonl";

/// Recorded events, by the topic they were sent on.
#[derive(Debug, Clone, Default)]
pub struct Recorded {
    by_topic: HashMap<String, Vec<Value>>,
    /// The time between two events of a topic after the first.
    pub pace: Duration,
}

impl Recorded {
    /// Nothing recorded, with a second between a topic's events.
    pub fn new() -> Self {
        Self { by_topic: HashMap::new(), pace: Duration::from_secs(1) }
    }

    /// Adds every `<topic>.jsonl` in `dir`. A topic recorded in two folders
    /// plays the later one.
    pub fn read(mut self, dir: &Path) -> Result<Self, String> {
        let problem = |e: std::io::Error| format!("can't read {}: {e}", dir.display());
        for entry in std::fs::read_dir(dir).map_err(problem)? {
            let path = entry.map_err(problem)?.path();
            let topic = path.file_stem().and_then(|stem| stem.to_str());
            if let (Some(topic), true) = (topic, path.extension().is_some_and(|ending| ending == RECORDING)) {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
                self = self.topic(topic, &text).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        Ok(self)
    }

    /// Sets `topic`'s events from `lines`, one event per line.
    pub fn topic(mut self, topic: &str, lines: &str) -> Result<Self, String> {
        let mut events = Vec::new();
        for (index, line) in lines.lines().enumerate().filter(|(_, line)| !line.trim().is_empty()) {
            let event: Value = serde_json::from_str(line).map_err(|e| format!("line {}: {e}", index + 1))?;
            if !event.get("event").is_some_and(Value::is_string) {
                return Err(format!("line {}: not an event (no \"event\" name)", index + 1));
            }
            events.push(event);
        }
        self.by_topic.insert(topic.to_string(), events);
        Ok(self)
    }
}

/// A fake novad listening on a socket until it is stopped or dropped.
pub struct FakeDaemon {
    shared: Arc<Shared>,
}

struct Shared {
    socket: PathBuf,
    recorded: Recorded,
    stopped: AtomicBool,
    /// The open connections, to hang up on when the fake stops.
    clients: Mutex<Vec<Arc<Client>>>,
}

impl FakeDaemon {
    /// Listens on `socket`, replacing a socket file nobody listens on.
    pub fn start(socket: &Path, recorded: Recorded) -> std::io::Result<Self> {
        if UnixStream::connect(socket).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("a novad is already listening on {}", socket.display()),
            ));
        }
        match std::fs::remove_file(socket) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let listener = UnixListener::bind(socket)?;
        let shared = Arc::new(Shared {
            socket: socket.to_path_buf(),
            recorded,
            stopped: AtomicBool::new(false),
            clients: Mutex::default(),
        });
        let on_thread = Arc::clone(&shared);
        std::thread::spawn(move || on_thread.serve(&listener));
        Ok(Self { shared })
    }

    /// Hangs up on every client and stops listening, as novad does when it
    /// exits. A fake started on the same socket afterwards is novad
    /// starting again.
    pub fn stop(self) {}
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        // Wakes the thread that accepts, which then sees it has stopped.
        drop(UnixStream::connect(&self.shared.socket));
        let _ = std::fs::remove_file(&self.shared.socket);
        for client in self.shared.clients.lock().unwrap_or_else(PoisonError::into_inner).drain(..) {
            client.hang_up();
        }
    }
}

impl Shared {
    /// Accepts clients until the fake stops; the connection made to wake it
    /// then ends the loop.
    fn serve(self: &Arc<Self>, listener: &UnixListener) {
        for stream in listener.incoming() {
            if self.stopped.load(Ordering::SeqCst) {
                return;
            }
            if let Ok(stream) = stream {
                self.accepted(stream);
            }
        }
    }

    fn accepted(self: &Arc<Self>, stream: UnixStream) {
        // A client that reads nothing must not hold the fake's threads.
        let Ok(reader) = stream.set_write_timeout(Some(WRITE_TIMEOUT)).and_then(|()| stream.try_clone()) else {
            return;
        };
        let client = Arc::new(Client { writer: Mutex::new(stream), gone: AtomicBool::new(false) });
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).push(Arc::clone(&client));
        // Stopped while this one was accepted: its hang-up was missed.
        if self.stopped.load(Ordering::SeqCst) {
            client.hang_up();
            return;
        }
        let shared = Arc::clone(self);
        std::thread::spawn(move || {
            client.send(&Event::Hello { daemon: VERSION.into() });
            let (mut reader, mut line) = (BufReader::new(reader), Vec::new());
            while let Ok(true) = read_line(&mut reader, REQUEST_MAX_BYTES, &mut line) {
                shared.answer(&client, String::from_utf8_lossy(&line).trim());
            }
            client.hang_up();
            let mut clients = shared.clients.lock().unwrap_or_else(PoisonError::into_inner);
            clients.retain(|each| !Arc::ptr_eq(each, &client));
        });
    }

    fn answer(&self, client: &Arc<Client>, line: &str) {
        let (id, topics) = match decode_request::<Command>(line) {
            Ok(request) => match request.command {
                Command::Subscribe { topics } => (request.id, topics),
                Command::Ping => return client.reply(request.id, Outcome::Result("pong".into())),
                _ => return client.reply(request.id, Outcome::Error(NOT_RECORDED.into())),
            },
            Err(_) => {
                let id = serde_json::from_str::<Value>(line).ok().and_then(|request| request.get("id")?.as_u64());
                return client.reply(id, Outcome::Error(NOT_RECORDED.into()));
            }
        };
        client.reply(id, Outcome::Result(Value::Null));
        for topic in topics {
            let Some(events) = self.recorded.by_topic.get(&topic) else { continue };
            let Some((first, later)) = events.split_first() else { continue };
            client.send(first);
            if !later.is_empty() {
                let (client, later, pace) = (Arc::clone(client), later.to_vec(), self.recorded.pace);
                std::thread::spawn(move || client.play(&later, pace));
            }
        }
    }
}

/// How long a client may take nothing the fake writes before it counts as
/// gone.
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const NOT_RECORDED: &str = "the fake novad only replays recorded events";

/// One connection to the fake.
struct Client {
    writer: Mutex<UnixStream>,
    gone: AtomicBool,
}

impl Client {
    fn send(&self, message: &impl Serialize) {
        let Ok(line) = encode(message) else { return };
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if writer.write_all(line.as_bytes()).is_err() {
            self.gone.store(true, Ordering::SeqCst);
        }
    }

    fn reply(&self, id: Option<u64>, outcome: Outcome) {
        if let Some(id) = id {
            self.send(&Response { id, outcome });
        }
    }

    /// Sends `events` one `pace` apart, until the client is gone.
    fn play(&self, events: &[Value], pace: Duration) {
        let (_never, wait) = mpsc::channel::<()>();
        for event in events {
            if wait.recv_timeout(pace) != Err(RecvTimeoutError::Timeout) || self.gone.load(Ordering::SeqCst) {
                return;
            }
            self.send(event);
        }
    }

    fn hang_up(&self) {
        self.gone.store(true, Ordering::SeqCst);
        let _ = self.writer.lock().unwrap_or_else(PoisonError::into_inner).shutdown(Shutdown::Both);
    }
}
