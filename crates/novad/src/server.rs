//! The socket: accepts clients, answers their requests and feeds them the
//! events they subscribed to.

use crate::Daemon;
use crate::hub::{ClientId, ClientQueue, Outgoing};
use nova_protocol::{Command, Event, Outcome, Response, Topic, encode};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;

/// Any local process can connect: longer request lines close the connection.
pub const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const SOCKET_DIR_MODE: u32 = 0o700;
const LOG_SOURCE: &str = "server";

pub fn bind(socket: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(SOCKET_DIR_MODE).create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(SOCKET_DIR_MODE))?;
    }
    if UnixStream::connect(socket).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another novad is already listening on {}", socket.display()),
        ));
    }
    // A socket file left by a previous run would make bind fail.
    match std::fs::remove_file(socket) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    UnixListener::bind(socket)
}

pub fn serve(listener: UnixListener, daemon: Arc<Daemon>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let daemon = Arc::clone(&daemon);
                std::thread::spawn(move || {
                    if let Err(e) = Connection::open(stream, &daemon).and_then(|c| c.run()) {
                        daemon.logger.warn(LOG_SOURCE, &format!("client error: {e}"));
                    }
                });
            }
            Err(e) => daemon.logger.warn(LOG_SOURCE, &format!("accept failed: {e}")),
        }
    }
}

struct Connection<'a> {
    daemon: &'a Daemon,
    id: ClientId,
    queue: ClientQueue,
    reader: BufReader<UnixStream>,
}

impl<'a> Connection<'a> {
    fn open(stream: UnixStream, daemon: &'a Daemon) -> std::io::Result<Self> {
        let mut writer = stream.try_clone()?;
        let (queue, outgoing) = ClientQueue::new(&stream)?;
        std::thread::spawn(move || {
            for message in outgoing {
                match message {
                    Outgoing::Line(line) => {
                        if writer.write_all(line.as_bytes()).is_err() {
                            return;
                        }
                    }
                    Outgoing::Flushed(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        let id = daemon.hub.register(queue.clone());
        let connection = Self { daemon, id, queue, reader: BufReader::new(stream) };
        connection.send(Event::Hello { daemon: env!("CARGO_PKG_VERSION").into() });
        Ok(connection)
    }

    fn run(mut self) -> std::io::Result<()> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = (&mut self.reader).take(MAX_REQUEST_BYTES + 1).read_until(b'\n', &mut line)?;
            if read == 0 {
                return Ok(());
            }
            if line.last() != Some(&b'\n') && read as u64 > MAX_REQUEST_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("request over {MAX_REQUEST_BYTES} bytes, closing"),
                ));
            }
            let text = String::from_utf8_lossy(&line);
            if !text.trim().is_empty() {
                self.handle(text.trim());
            }
        }
    }

    fn handle(&self, line: &str) {
        let request = match nova_protocol::decode_request(line) {
            Ok(request) => request,
            Err(e) => {
                self.daemon.logger.warn(LOG_SOURCE, &format!("ignoring bad request: {e}"));
                if let Some(id) = request_id(line) {
                    self.send(Response { id, outcome: Outcome::Error(format!("bad request: {e}")) });
                }
                return;
            }
        };
        // Answered even without an id: some commands act as they answer.
        let outcome = self.answer(&request.command);
        if let Some(id) = request.id {
            self.send(Response { id, outcome });
        }
        self.follow_up(request.command);
    }

    fn answer(&self, command: &Command) -> Outcome {
        let value = match command {
            Command::Ping => Ok("pong".into()),
            Command::ListFeatures => serde_json::to_value(&self.daemon.features),
            Command::Status => serde_json::to_value(self.daemon.status()),
            Command::Subscribe { .. } | Command::Quit => Ok(serde_json::Value::Null),
            Command::RefreshAiUsage => {
                return self.with_ai_usage(|usage| {
                    usage.refresh();
                    Ok(())
                });
            }
            Command::ShowAiUsage { provider } => {
                return self.with_ai_usage(|usage| usage.show(*provider));
            }
            Command::ReportClaudeUsage { limits } => {
                return self.with_ai_usage(|usage| usage.report_claude(limits.clone()));
            }
        };
        match value {
            Ok(value) => Outcome::Result(value),
            Err(e) => Outcome::Error(e.to_string()),
        }
    }

    fn with_ai_usage(&self, act: impl FnOnce(&crate::ai_usage::Usage) -> Result<(), String>) -> Outcome {
        match &self.daemon.ai_usage {
            Some(usage) => match act(usage) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error(format!("the {} feature is off", crate::ai_usage::FEATURE_ID)),
        }
    }

    /// Work done after the reply is queued.
    fn follow_up(&self, command: Command) {
        match command {
            Command::Subscribe { topics } => {
                for topic in topics {
                    match topic {
                        Topic::Features => {
                            self.daemon.hub.subscribe(self.id, topic);
                            self.send(Event::Features { features: self.daemon.features.clone() });
                        }
                        Topic::Logs => self.daemon.logger.subscribe(self.id),
                        Topic::Theme => {
                            self.daemon.hub.subscribe(self.id, topic);
                            self.send(Event::Theme(self.daemon.theme.clone()));
                        }
                        // Without the feature there are no events to send.
                        Topic::AiUsage => {
                            if let Some(usage) = &self.daemon.ai_usage {
                                self.daemon.hub.subscribe(self.id, topic);
                                self.send(usage.event());
                            }
                        }
                    }
                }
            }
            Command::Quit => {
                self.flush();
                self.daemon.quit();
            }
            Command::Ping
            | Command::ListFeatures
            | Command::Status
            | Command::ShowAiUsage { .. }
            | Command::RefreshAiUsage
            | Command::ReportClaudeUsage { .. } => {}
        }
    }

    fn send(&self, message: impl serde::Serialize) {
        match encode(message) {
            Ok(line) => {
                self.queue.push(Outgoing::Line(line));
            }
            Err(e) => self.daemon.logger.error(LOG_SOURCE, &format!("can't encode a message: {e}")),
        }
    }

    /// Waits until everything queued so far is written.
    fn flush(&self) {
        let (done, flushed) = mpsc::sync_channel(1);
        if self.queue.push(Outgoing::Flushed(done)) {
            let _ = flushed.recv();
        }
    }
}

impl Drop for Connection<'_> {
    fn drop(&mut self) {
        self.daemon.hub.unregister(self.id);
    }
}

/// The `id` of a request that didn't parse, so the error can be answered.
fn request_id(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line).ok()?.get("id")?.as_u64()
}
