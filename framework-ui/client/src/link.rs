//! The connection to novad's socket, without GPUI: one thread that connects,
//! reads novad's lines and connects again when novad goes away. Requests are
//! written from whichever thread makes them.
//!
//! ```text
//! novad -> ui   {"v":2,"event":"hello","data":{"daemon":"0.1.0"}}
//! ui -> novad   {"v":2,"id":1,"cmd":"subscribe","args":{"topics":["sound"]}}
//! novad -> ui   {"v":2,"id":1,"result":null}
//! novad -> ui   {"v":2,"event":"sound","data":{...}}
//! ```
//!
//! What arrives goes to the link's sink as an [`Incoming`], in the order
//! novad sent it; [`crate::Daemon`] is the sink that carries it to GPUI.

use nova_carrier::{Envelope, PROTOCOL_VERSION, Request, Topic, encode};
use nova_daemon_info::{Command, Event};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// How long the link waits before it tries novad's socket again. novad may
/// start after the UI, or restart.
pub const RETRY: Duration = if cfg!(test) { Duration::from_millis(20) } else { Duration::from_secs(1) };
/// How long novad may take to say hello on a new connection.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest line novad may send: a longer one ends the connection. Far
/// above any event (the largest carry a list of apps or log entries), so
/// only a broken sender gets there.
const LINE_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// How long a request's write may make no progress. Requests are written on
/// the caller's thread, the UI's, so a novad that reads nothing must not
/// hold it for long: the link hangs up and connects again.
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// What the link tells its sink.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// novad said hello: requests go through from here on, and the topics
    /// asked for are subscribed to again.
    Connected { daemon: String },
    /// The connection ended. The link is trying again.
    Lost,
    /// An event, as novad named it, with its `data` (null when it has none).
    Event { name: String, data: Value },
}

/// Why a request has no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There was no connection to send it on.
    NotConnected,
    /// The connection ended before novad answered.
    WentAway,
    /// novad took too long to answer.
    TimedOut,
    /// novad answered with this error.
    Daemon(String),
    /// The request couldn't be written as JSON, or the result wasn't the
    /// type asked for.
    Unreadable(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "not connected to novad"),
            Self::WentAway => write!(f, "novad went away"),
            Self::TimedOut => write!(f, "novad didn't answer in time"),
            Self::Daemon(error) => write!(f, "novad: {error}"),
            Self::Unreadable(problem) => write!(f, "{problem}"),
        }
    }
}

/// What gets a request's result, once.
type Reply = Box<dyn FnOnce(Result<Value, Refusal>) + Send>;

/// The connection to novad at one socket path. Dropping it hangs up and
/// ends its thread.
pub struct Link {
    shared: Arc<Shared>,
}

struct Shared {
    socket: PathBuf,
    state: Mutex<State>,
    /// Wakes the wait between two tries when the link closes.
    closing: Condvar,
    sink: Box<dyn Fn(Incoming) + Send + Sync>,
}

#[derive(Default)]
struct State {
    /// The connection requests are written to, once novad said hello.
    writer: Option<UnixStream>,
    next_id: u64,
    /// The topics asked for, each with how many asked, subscribed to on
    /// every connection.
    topics: Vec<(Topic, usize)>,
    /// The requests novad hasn't answered yet, by id.
    pending: HashMap<u64, Reply>,
    /// Why the last try found a novad it couldn't talk to, so the same
    /// problem is logged once, not on every try.
    problem: Option<String>,
    closed: bool,
}

impl Link {
    /// Connects to novad at `socket` and keeps connecting, on a thread of
    /// its own. `sink` is called on that thread and must not call the link
    /// back: it only hands the message on.
    pub fn open(socket: PathBuf, sink: impl Fn(Incoming) + Send + Sync + 'static) -> Self {
        let shared =
            Arc::new(Shared { socket, state: Mutex::default(), closing: Condvar::new(), sink: Box::new(sink) });
        let on_thread = Arc::clone(&shared);
        std::thread::spawn(move || on_thread.run());
        Self { shared }
    }

    /// Asks novad for `topic`'s events, now when connected and again on
    /// every later connection. novad answers a subscription with what the
    /// topic shows now, so asking for a topic again sends its state again:
    /// what a view that starts later needs. Each topic is its own request,
    /// so one that this novad doesn't know costs only itself.
    pub fn subscribe(&self, topic: Topic) {
        let mut state = self.shared.lock();
        match state.topics.iter_mut().find(|(each, _)| *each == topic) {
            Some((_, askers)) => *askers += 1,
            None => state.topics.push((topic, 1)),
        }
        state.subscribe(topic);
    }

    /// Takes back one [`Link::subscribe`] of `topic`. Once nobody asks for
    /// it, later connections leave it out. novad has no way to stop a topic
    /// on a connection that has it, so until then its events still arrive.
    pub fn unsubscribe(&self, topic: Topic) {
        let mut state = self.shared.lock();
        if let Some((_, askers)) = state.topics.iter_mut().find(|(each, _)| *each == topic) {
            *askers = askers.saturating_sub(1);
        }
        state.topics.retain(|(_, askers)| *askers > 0);
    }

    /// Sends `command` to novad. `reply` gets its result, or why there is
    /// none; it is called once, on the caller's thread when the request
    /// can't be sent and on the link's otherwise.
    pub fn request(&self, command: impl Serialize, reply: impl FnOnce(Result<Value, Refusal>) + Send + 'static) {
        let refused = self.shared.lock().request(command, Box::new(reply));
        if let Some((reply, refusal)) = refused {
            reply(Err(refusal));
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.closed = true;
        state.hang_up();
        self.shared.closing.notify_all();
    }
}

impl State {
    /// Writes `command` as a request and keeps `reply` for its answer.
    /// Gives `reply` back with the reason when it couldn't be sent.
    fn request(&mut self, command: impl Serialize, reply: Reply) -> Option<(Reply, Refusal)> {
        let Some(writer) = &mut self.writer else { return Some((reply, Refusal::NotConnected)) };
        self.next_id += 1;
        let id = self.next_id;
        let line = match encode(Request { id: Some(id), command }) {
            Ok(line) => line,
            Err(e) => return Some((reply, Refusal::Unreadable(format!("can't write the request: {e}")))),
        };
        if writer.write_all(line.as_bytes()).is_err() {
            // The reader's side ends too, which starts the next try.
            self.hang_up();
            return Some((reply, Refusal::WentAway));
        }
        self.pending.insert(id, reply);
        None
    }

    fn subscribe(&mut self, topic: Topic) {
        let name = topic.name();
        let refused = self.request(
            Command::Subscribe { topics: vec![name.to_string()] },
            Box::new(move |result| {
                if let Err(refusal) = result {
                    crate::log::warn(&format!("no {name} events: {refusal}"));
                }
            }),
        );
        // Not connected: the next connection subscribes.
        drop(refused);
    }

    fn hang_up(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.shutdown(Shutdown::Both);
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // The state stays usable after a panic elsewhere: every change to it
        // is complete before its lock is let go.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Connects, reads until the connection ends and tries again, until the
    /// link closes.
    fn run(&self) {
        loop {
            if self.lock().closed {
                return;
            }
            if let Some(reader) = self.connect() {
                self.read(reader);
                self.lost();
            }
            let state = self.lock();
            let waited = self.closing.wait_timeout_while(state, RETRY, |state| !state.closed);
            if waited.unwrap_or_else(PoisonError::into_inner).0.closed {
                return;
            }
        }
    }

    /// One try: a connection that said hello, with the topics subscribed
    /// to. Nothing is said of a novad that isn't there, since that's every
    /// try while it's down.
    fn connect(&self) -> Option<BufReader<UnixStream>> {
        let stream = UnixStream::connect(&self.socket).ok()?;
        let greeted = self.greet(stream);
        let mut state = self.lock();
        match greeted {
            Ok(reader) => {
                state.problem = None;
                Some(reader)
            }
            Err(problem) => {
                if !state.closed && state.problem.as_ref() != Some(&problem) {
                    crate::log::warn(&format!("can't talk to novad at {}: {problem}", self.socket.display()));
                    state.problem = Some(problem);
                }
                None
            }
        }
    }

    fn greet(&self, stream: UnixStream) -> Result<BufReader<UnixStream>, String> {
        let io = |e: std::io::Error| e.to_string();
        stream.set_read_timeout(Some(HELLO_TIMEOUT)).map_err(io)?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(io)?;
        let writer = stream.try_clone().map_err(io)?;
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        read_line(&mut reader, LINE_MAX_BYTES, &mut line).map_err(|e| format!("no hello: {e}"))?;
        let daemon = match serde_json::from_slice::<Envelope<Event>>(&line) {
            Ok(Envelope { v: PROTOCOL_VERSION, body: Event::Hello { daemon } }) => daemon,
            Ok(Envelope { v, .. }) if v != PROTOCOL_VERSION => {
                return Err(format!("it speaks protocol {v}, this UI {PROTOCOL_VERSION}"));
            }
            _ => return Err("it didn't say hello".into()),
        };
        reader.get_ref().set_read_timeout(None).map_err(io)?;
        let mut state = self.lock();
        if state.closed {
            return Err("the link closed".into());
        }
        state.writer = Some(writer);
        // Told before the subscriptions are written, and under the lock, so
        // the sink hears of the connection before anything sent on it.
        (self.sink)(Incoming::Connected { daemon });
        for (topic, _) in state.topics.clone() {
            state.subscribe(topic);
        }
        Ok(reader)
    }

    /// Reads novad's lines until the connection ends.
    fn read(&self, mut reader: BufReader<UnixStream>) {
        let mut line = Vec::new();
        loop {
            match read_line(&mut reader, LINE_MAX_BYTES, &mut line) {
                Ok(true) => self.take(String::from_utf8_lossy(&line).trim()),
                Ok(false) => return,
                Err(e) => return crate::log::warn(&format!("hanging up on novad: {e}")),
            }
        }
    }

    /// One line from novad: an event for the sink, or a request's answer.
    fn take(&self, line: &str) {
        if line.is_empty() {
            return;
        }
        match serde_json::from_str::<Line>(line).map_err(|e| e.to_string()).and_then(Line::read) {
            Ok(Message::Event { name, data }) => (self.sink)(Incoming::Event { name, data }),
            Ok(Message::Answer { id, result }) => {
                let reply = self.lock().pending.remove(&id);
                if let Some(reply) = reply {
                    reply(result);
                }
            }
            Err(e) => crate::log::warn(&format!("ignoring a line from novad: {e}")),
        }
    }

    /// The connection ended: nothing answers the requests still waiting.
    fn lost(&self) {
        let pending = {
            let mut state = self.lock();
            state.hang_up();
            std::mem::take(&mut state.pending)
        };
        for reply in pending.into_values() {
            reply(Err(Refusal::WentAway));
        }
        (self.sink)(Incoming::Lost);
    }
}

/// A line novad sends after its hello, exactly as it may arrive: no other
/// fields, as novad reads its requests (`nova_carrier::envelope`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    v: u32,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default, deserialize_with = "present")]
    data: Option<Value>,
}

/// A field that is there, also when it holds `null` (a result often does).
fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// What a line says.
enum Message {
    Event { name: String, data: Value },
    Answer { id: u64, result: Result<Value, Refusal> },
}

impl Line {
    /// An event, or a request's answer with its result or its error;
    /// anything else is no message.
    fn read(self) -> Result<Message, String> {
        if self.v != PROTOCOL_VERSION {
            return Err(format!("protocol version {}, this UI speaks {PROTOCOL_VERSION}", self.v));
        }
        match (self.event, self.id, self.result, self.error, self.data) {
            (Some(name), None, None, None, data) => Ok(Message::Event { name, data: data.unwrap_or(Value::Null) }),
            (None, Some(id), Some(result), None, None) => Ok(Message::Answer { id, result: Ok(result) }),
            (None, Some(id), None, Some(error), None) => {
                Ok(Message::Answer { id, result: Err(Refusal::Daemon(error)) })
            }
            _ => Err("neither an event nor an answer".into()),
        }
    }
}

/// Reads one line of at most `limit` bytes into `line`, replacing what it
/// held. False at the end of the stream; a longer line is an error, so a
/// sender can't make its reader hold an endless one.
pub(crate) fn read_line(reader: &mut impl BufRead, limit: u64, line: &mut Vec<u8>) -> std::io::Result<bool> {
    line.clear();
    let read = reader.by_ref().take(limit + 1).read_until(b'\n', line)?;
    if line.last() != Some(&b'\n') && read as u64 > limit {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("a line over {limit} bytes")));
    }
    Ok(read > 0)
}
