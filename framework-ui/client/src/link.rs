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

pub const RETRY: Duration = if cfg!(test) { Duration::from_millis(20) } else { Duration::from_secs(1) };
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const LINE_MAX_BYTES: u64 = 16 * 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Connected { daemon: String },
    Lost,
    Event { name: String, data: Value },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotConnected,
    WentAway,
    TimedOut,
    Daemon(String),
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

type Reply = Box<dyn FnOnce(Result<Value, Refusal>) + Send>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked(u64);

pub struct Link {
    shared: Arc<Shared>,
}

struct Shared {
    socket: PathBuf,
    state: Mutex<State>,
    closing: Condvar,
    sink: Box<dyn Fn(Incoming) + Send + Sync>,
}

#[derive(Default)]
struct State {
    writer: Option<UnixStream>,
    next_id: u64,
    topics: Vec<(Topic, usize)>,
    pending: HashMap<u64, Reply>,
    problem: Option<String>,
    closed: bool,
}

impl Link {
    pub fn open(socket: PathBuf, sink: impl Fn(Incoming) + Send + Sync + 'static) -> Self {
        let shared =
            Arc::new(Shared { socket, state: Mutex::default(), closing: Condvar::new(), sink: Box::new(sink) });
        let on_thread = Arc::clone(&shared);
        std::thread::spawn(move || on_thread.run());
        Self { shared }
    }

    pub fn subscribe(&self, topic: Topic) {
        let mut state = self.shared.lock();
        match state.topics.iter_mut().find(|(each, _)| *each == topic) {
            Some((_, askers)) => *askers += 1,
            None => state.topics.push((topic, 1)),
        }
        state.subscribe(topic);
    }

    pub fn unsubscribe(&self, topic: Topic) {
        let mut state = self.shared.lock();
        if let Some((_, askers)) = state.topics.iter_mut().find(|(each, _)| *each == topic) {
            *askers = askers.saturating_sub(1);
        }
        state.topics.retain(|(_, askers)| *askers > 0);
    }

    pub fn request(
        &self,
        command: impl Serialize,
        reply: impl FnOnce(Result<Value, Refusal>) + Send + 'static,
    ) -> Option<Asked> {
        let asked = self.shared.lock().request(command, Box::new(reply));
        match asked {
            Ok(id) => Some(Asked(id)),
            Err((reply, refusal)) => {
                reply(Err(refusal));
                None
            }
        }
    }

    pub fn forget(&self, asked: Asked) {
        let forgotten = self.shared.lock().pending.remove(&asked.0);
        drop(forgotten);
    }

    #[cfg(test)]
    pub fn waiting(&self) -> usize {
        self.shared.lock().pending.len()
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
    fn request(&mut self, command: impl Serialize, reply: Reply) -> Result<u64, (Reply, Refusal)> {
        let Some(writer) = &mut self.writer else { return Err((reply, Refusal::NotConnected)) };
        self.next_id += 1;
        let id = self.next_id;
        let line = match encode(Request { id: Some(id), command }) {
            Ok(line) => line,
            Err(e) => return Err((reply, Refusal::Unreadable(format!("can't write the request: {e}")))),
        };
        if writer.write_all(line.as_bytes()).is_err() {
            self.hang_up();
            return Err((reply, Refusal::WentAway));
        }
        self.pending.insert(id, reply);
        Ok(id)
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
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
        (self.sink)(Incoming::Connected { daemon });
        for (topic, _) in state.topics.clone() {
            state.subscribe(topic);
        }
        Ok(reader)
    }

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

fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

enum Message {
    Event { name: String, data: Value },
    Answer { id: u64, result: Result<Value, Refusal> },
}

impl Line {
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

pub(crate) fn read_line(reader: &mut impl BufRead, limit: u64, line: &mut Vec<u8>) -> std::io::Result<bool> {
    line.clear();
    let read = reader.by_ref().take(limit + 1).read_until(b'\n', line)?;
    if line.last() != Some(&b'\n') && read as u64 > limit {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("a line over {limit} bytes")));
    }
    Ok(read > 0)
}
