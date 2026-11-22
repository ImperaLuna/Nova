//! A connection to novad's socket. Every read and write gives up after
//! [`REPLY_TIMEOUT`], so a stuck novad can't hang the CLI (`nova restart` is
//! what you run then); only following events (`nova logs`, `nova ipc
//! subscribe`) waits forever.

use crate::lifecycle::RESTART_COMMAND;
use nova_protocol::{Command, Envelope, Event, Outcome, Response, Status, Topic, encode};
use serde::{Deserialize, Serialize};
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

/// Why there's no client: no novad listening, or one that doesn't answer
/// properly (stuck, another version, a socket it can't be reached on).
#[derive(Debug)]
pub enum Unreachable {
    NotRunning,
    NoAnswer(String),
}

impl Unreachable {
    /// What a command that needs novad tells the user.
    pub fn advice(&self) -> String {
        match self {
            Self::NotRunning => "Nova isn't running. Start it with `nova`.".into(),
            Self::NoAnswer(problem) => {
                format!("Nova's daemon doesn't answer ({problem}). Run `{RESTART_COMMAND}` to start it again.")
            }
        }
    }
}

impl Client {
    /// Connects, or says what to do about it.
    pub fn connect(socket: &Path) -> Result<Self, String> {
        Self::reach(socket).map_err(|unreachable| unreachable.advice())
    }

    /// Connects once, telling "not running" from "stuck" by how that
    /// connection fails.
    pub fn reach(socket: &Path) -> Result<Self, Unreachable> {
        use std::io::ErrorKind;
        match UnixStream::connect(socket) {
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
                Err(Unreachable::NotRunning)
            }
            Err(e) => Err(Unreachable::NoAnswer(format!("can't reach novad at {}: {e}", socket.display()))),
            Ok(stream) => Self::over(stream).map_err(Unreachable::NoAnswer),
        }
    }

    /// A client on a stream that's already connected to novad's socket.
    pub fn over(stream: UnixStream) -> Result<Self, String> {
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut client = Self { reader: BufReader::new(stream), writer, next_id: 1, daemon_version: String::new() };
        match client.next_message()? {
            Message::Event(Sent::Shared(Event::Hello { daemon })) => client.daemon_version = daemon,
            _ => return Err("novad didn't say hello; is it a different version?".into()),
        }
        Ok(client)
    }

    pub fn status(&mut self) -> Result<Status, String> {
        let value = self.request(Command::Status)?;
        serde_json::from_value(value).map_err(|e| format!("novad sent an odd status: {e}"))
    }

    /// Sends `command` and returns its result. Events that arrive first are dropped.
    pub fn request(&mut self, command: impl Serialize) -> Result<serde_json::Value, String> {
        self.request_seeing_events(command, |_| {})
    }

    /// Subscribes to `topics` and returns what novad sends for them now. It
    /// sends that after its reply, and a topic whose feature is off sends
    /// nothing, so an empty subscribe after it marks where the state ends.
    pub fn current(&mut self, topics: Vec<Topic>) -> Result<Vec<Sent>, String> {
        self.request(Command::Subscribe { topics: topics.iter().map(|topic| topic.name().to_string()).collect() })?;
        let mut events = Vec::new();
        self.request_seeing_events(Command::Subscribe { topics: vec![] }, |event| events.push(event))?;
        Ok(events)
    }

    /// Sends `command` and returns its result, passing events that arrive
    /// first to `event`.
    fn request_seeing_events(
        &mut self,
        command: impl Serialize,
        mut event: impl FnMut(Sent),
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = encode(nova_carrier::Request { id: Some(id), command }).map_err(|e| e.to_string())?;
        self.writer.write_all(line.as_bytes()).map_err(|e| format!("can't send to novad: {e}"))?;
        loop {
            match self.next_message()? {
                Message::Reply(Response { id: reply_id, outcome }) if reply_id == id => {
                    return match outcome {
                        Outcome::Result(result) => Ok(result),
                        Outcome::Error(error) => Err(format!("novad: {error}")),
                    };
                }
                Message::Event(sent) => event(sent),
                Message::Reply(_) => {}
            }
        }
    }

    /// Waits for events as long as novad runs, for `next_event`. Requests
    /// still time out on writing.
    pub fn follow(&mut self) -> Result<(), String> {
        self.reader.get_ref().set_read_timeout(None).map_err(|e| e.to_string())
    }

    /// The next event; replies are skipped. `None` when novad hangs up.
    pub fn next_event(&mut self) -> Result<Option<Sent>, String> {
        loop {
            match self.next_message() {
                Ok(Message::Event(event)) => return Ok(Some(event)),
                Ok(Message::Reply(_)) => {}
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
        if let Ok(event) = serde_json::from_str::<Envelope<Sent>>(&line) {
            return Ok(Message::Event(event.body));
        }
        let reply = serde_json::from_str::<Envelope<Response>>(&line)
            .map_err(|e| format!("novad sent something unreadable: {e}"))?;
        Ok(Message::Reply(reply.body))
    }
}

const HUNG_UP: &str = "novad closed the connection";

/// An event novad sends: a shared one, or a moved feature's own (its api
/// crate's `Event`, one variant per feature, sorted by feature id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Sent {
    Shared(Event),
    AiUsage(nova_ai_usage_api::Event),
    Launcher(nova_launcher_api::Event),
    Lock(nova_lock_api::Event),
    Media(nova_media_api::Event),
    Network(nova_network_api::Event),
    Notifications(nova_notifications_api::Event),
    Peripherals(nova_peripherals_api::Event),
    Recording(nova_recording_api::Event),
    Resources(nova_resources_api::Event),
    Screenshot(nova_screenshot_api::Event),
    Tray(nova_tray_api::Event),
}

enum Message {
    Event(Sent),
    Reply(Response),
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;
    use std::os::unix::net::UnixListener;

    /// A novad that says hello, then answers the first request with `reply`
    /// (`{id}` is the request's id).
    fn answering(dir: &TempDir, reply: &'static str) -> std::path::PathBuf {
        let socket = dir.join("novad.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            writer.write_all(encode(Event::Hello { daemon: "test".into() }).unwrap().as_bytes()).unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let id = serde_json::from_str::<serde_json::Value>(&request).unwrap()["id"].as_u64().unwrap();
            writer.write_all(format!("{}\n", reply.replace("{id}", &id.to_string())).as_bytes()).unwrap();
            // Held open until the client hangs up.
            let _ = reader.read_line(&mut request);
        });
        socket
    }

    /// A moved feature's events read like the shared ones (`nova ipc
    /// subscribe` prints them as novad sent them).
    #[test]
    fn feature_events_are_read() {
        let line = r#"{"v":2,"event":"resources","data":{"cpu":1.0,"memory_used":1,"memory_total":2,"history":[],"history_size":36,"apps":[]}}"#;
        let sent = serde_json::from_str::<Envelope<Sent>>(line).unwrap().body;
        assert!(matches!(sent, Sent::Resources(nova_resources_api::Event::Resources(_))), "{sent:?}");
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let expected = serde_json::json!({"event": "resources", "data": value["data"]});
        assert_eq!(serde_json::to_value(&sent).unwrap(), expected);
        let line = r#"{"v":2,"event":"network_traffic","data":{"down":[1],"up":[2],"today_down":3,"today_up":4}}"#;
        let sent = serde_json::from_str::<Envelope<Sent>>(line).unwrap().body;
        assert!(matches!(sent, Sent::Network(nova_network_api::Event::NetworkTraffic(_))), "{sent:?}");
    }

    #[test]
    fn replies_are_typed() {
        let dir = TempDir::new("client-replies");
        let socket = answering(&dir, r#"{"v":1,"id":{id},"result":"pong"}"#);
        assert_eq!(Client::connect(&socket).unwrap().request(Command::Ping), Ok("pong".into()));

        let dir = TempDir::new("client-error");
        let socket = answering(&dir, r#"{"v":1,"id":{id},"error":"no such app"}"#);
        assert_eq!(Client::connect(&socket).unwrap().request(Command::Ping), Err("novad: no such app".into()));

        // Neither a result nor an error, or an error that isn't text, is no
        // success.
        for (name, odd) in [("client-empty", r#"{"v":1,"id":{id}}"#), ("client-odd", r#"{"v":1,"id":{id},"error":3}"#)]
        {
            let dir = TempDir::new(name);
            let socket = answering(&dir, odd);
            let answer = Client::connect(&socket).unwrap().request(Command::Ping);
            assert!(answer.as_ref().is_err_and(|e| e.contains("unreadable")), "{odd}: {answer:?}");
        }
    }

    /// Not running, and running but not answering, are told apart.
    #[test]
    fn a_silent_novad_times_out() {
        let dir = TempDir::new("client-silent");
        let socket = dir.join("novad.sock");
        assert!(matches!(Client::reach(&socket), Err(Unreachable::NotRunning)));
        let _listener = UnixListener::bind(&socket).unwrap();
        let started = std::time::Instant::now();
        let Err(Unreachable::NoAnswer(problem)) = Client::reach(&socket) else { panic!("it answered") };
        assert!(started.elapsed() < REPLY_TIMEOUT * 2);
        let advice = Unreachable::NoAnswer(problem).advice();
        assert!(advice.contains("doesn't answer") && advice.contains(RESTART_COMMAND), "{advice}");
    }
}
