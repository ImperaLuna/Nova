//! A connection to novad's socket. Every read and write gives up after
//! [`REPLY_TIMEOUT`], so a stuck novad can't hang the CLI (`nova restart` is
//! what you run then); only following events (`nova logs`) waits forever.

use nova_protocol::{Command, Envelope, Event, Request, Status, Topic, encode};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// How long novad may take to say hello or answer a request.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// novad's version, from its hello.
    pub daemon_version: String,
}

impl Client {
    pub fn connect(socket: &Path) -> Result<Self, String> {
        let stream =
            UnixStream::connect(socket).map_err(|e| format!("can't reach novad at {}: {e}", socket.display()))?;
        Self::over(stream)
    }

    /// A client on a stream that's already connected to novad's socket.
    pub fn over(stream: UnixStream) -> Result<Self, String> {
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut client = Self { reader: BufReader::new(stream), writer, next_id: 1, daemon_version: String::new() };
        match client.next_message()? {
            Message::Event(Event::Hello { daemon }) => client.daemon_version = daemon,
            _ => return Err("novad didn't say hello; is it a different version?".into()),
        }
        Ok(client)
    }

    pub fn status(&mut self) -> Result<Status, String> {
        let value = self.request(Command::Status)?;
        serde_json::from_value(value).map_err(|e| format!("novad sent an odd status: {e}"))
    }

    /// Sends `command` and returns its result. Events that arrive first are dropped.
    pub fn request(&mut self, command: Command) -> Result<serde_json::Value, String> {
        self.request_seeing_events(command, |_| {})
    }

    /// Subscribes to `topics` and returns what novad sends for them now. It
    /// sends that after its reply, and a topic whose feature is off sends
    /// nothing, so an empty subscribe after it marks where the state ends.
    pub fn current(&mut self, topics: Vec<Topic>) -> Result<Vec<Event>, String> {
        self.request(Command::Subscribe { topics })?;
        let mut events = Vec::new();
        self.request_seeing_events(Command::Subscribe { topics: vec![] }, |event| events.push(event))?;
        Ok(events)
    }

    /// Sends `command` and returns its result, passing events that arrive
    /// first to `event`.
    fn request_seeing_events(
        &mut self,
        command: Command,
        mut event: impl FnMut(Event),
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = encode(Request { id: Some(id), command }).map_err(|e| e.to_string())?;
        self.writer.write_all(line.as_bytes()).map_err(|e| format!("can't send to novad: {e}"))?;
        loop {
            match self.next_message()? {
                Message::Reply { id: reply_id, body } if reply_id == id => {
                    return match (body.get("error"), body.get("result")) {
                        (Some(error), _) => Err(format!("novad: {}", error.as_str().unwrap_or_default())),
                        (None, result) => Ok(result.cloned().unwrap_or_default()),
                    };
                }
                Message::Event(sent) => event(sent),
                Message::Reply { .. } => {}
            }
        }
    }

    /// Waits for events as long as novad runs, for `next_event`. Requests
    /// still time out on writing.
    pub fn follow(&mut self) -> Result<(), String> {
        self.reader.get_ref().set_read_timeout(None).map_err(|e| e.to_string())
    }

    /// The next event; replies are skipped. `None` when novad hangs up.
    pub fn next_event(&mut self) -> Result<Option<Event>, String> {
        loop {
            match self.next_message() {
                Ok(Message::Event(event)) => return Ok(Some(event)),
                Ok(Message::Reply { .. }) => {}
                Err(e) if e == HUNG_UP => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    fn next_message(&mut self) -> Result<Message, String> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).map_err(|e| format!("can't read from novad: {e}"))?;
        if read == 0 {
            return Err(HUNG_UP.into());
        }
        if let Ok(event) = serde_json::from_str::<Envelope<Event>>(&line) {
            return Ok(Message::Event(event.body));
        }
        let body: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| format!("novad sent something unreadable: {e}"))?;
        let id = body.get("id").and_then(|id| id.as_u64()).ok_or("novad sent a message with no id")?;
        Ok(Message::Reply { id, body })
    }
}

const HUNG_UP: &str = "novad closed the connection";

enum Message {
    Event(Event),
    Reply { id: u64, body: serde_json::Value },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A novad that accepts connections but never answers.
    #[test]
    fn a_silent_novad_times_out() {
        let dir = std::env::temp_dir().join(format!("nova-client-silent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("novad.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let started = std::time::Instant::now();
        assert!(Client::connect(&socket).is_err());
        assert!(started.elapsed() < REPLY_TIMEOUT * 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
