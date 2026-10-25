//! The socket: accepts clients, answers their requests and feeds them the
//! events they subscribed to.

use crate::hub::{ClientId, ClientQueue, Outgoing};
use crate::{Daemon, lock};
use nova_protocol::{Command, Event, FeatureStatus, Outcome, Panel, Response, Topic, encode};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

/// Any local process can connect: longer request lines close the connection.
pub const MAX_REQUEST_BYTES: u64 = 64 * 1024;
/// How long a write may make no progress before the client counts as stalled
/// and is disconnected. Only a client that stops reading with its socket
/// buffer full gets there; the UI reconnects on its own.
pub const WRITE_TIMEOUT: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(10) };
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
        let writer = stream.try_clone()?;
        writer.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let (queue, outgoing) = ClientQueue::new(&stream)?;
        let logger = Arc::clone(&daemon.logger);
        std::thread::spawn(move || write_all_queued(writer, outgoing, &logger));
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
            Command::ListFeatures => serde_json::to_value(&lock(&self.daemon.settings).features),
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
            Command::FocusWorkspace { id } => {
                return self.with_compositor(|compositor| compositor.focus_workspace(*id));
            }
            Command::StepWorkspace { output, step } => {
                return self.with_compositor(|compositor| compositor.step_workspace(output, *step));
            }
            Command::Toggle { panel } => return self.toggle(*panel),
            Command::Lock => {
                if !self.feature_ready(crate::session_lock::FEATURE_ID) {
                    return Outcome::Error(format!(
                        "the {} feature isn't ready (see `nova status`)",
                        crate::session_lock::FEATURE_ID
                    ));
                }
                return match self.daemon.session_lock.lock(&self.daemon.hub) {
                    Ok(()) => Outcome::Result(serde_json::Value::Null),
                    Err(e) => Outcome::Error(e),
                };
            }
            Command::ReportLocked => {
                self.daemon.session_lock.secured();
                Ok(serde_json::Value::Null)
            }
            // Even with the feature off: the screen was locked while it was on.
            Command::ReportUnlocked => {
                self.daemon.session_lock.unlocked(&self.daemon.hub);
                Ok(serde_json::Value::Null)
            }
        };
        match value {
            Ok(value) => Outcome::Result(value),
            Err(e) => Outcome::Error(e.to_string()),
        }
    }

    fn with_ai_usage(&self, act: impl FnOnce(&crate::ai_usage::Usage) -> Result<(), String>) -> Outcome {
        // Out of the lock: acting may take a while, and a reload may stop it
        // meanwhile (it then says the feature is off).
        let usage = lock(&self.daemon.settings).ai_usage.clone();
        match usage {
            Some(usage) => match act(&usage) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error(format!("the {} feature is off", crate::ai_usage::FEATURE_ID)),
        }
    }

    fn with_compositor(&self, act: impl FnOnce(&crate::compositor::Compositor) -> Result<(), String>) -> Outcome {
        match &self.daemon.compositor {
            Some(compositor) => match act(compositor) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error("novad doesn't know this compositor".into()),
        }
    }

    /// Tells the UI to open or close `panel` on the focused output. The UI
    /// knows which screen has it open, so it decides which.
    fn feature_ready(&self, id: &str) -> bool {
        lock(&self.daemon.settings)
            .features
            .iter()
            .any(|feature| feature.id == id && feature.status == FeatureStatus::Ready)
    }

    fn toggle(&self, panel: Panel) -> Outcome {
        if !self.feature_ready(panel.name()) {
            return Outcome::Error(format!("the {panel} feature isn't ready (see `nova status`)"));
        }
        let output = self.daemon.compositor.as_ref().and_then(|compositor| compositor.focused_output());
        match self.daemon.hub.publish(&Event::Toggle { panel, output }) {
            0 => Outcome::Error("the UI isn't connected to novad, so there's nothing to toggle".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        }
    }

    /// Work done after the reply is queued.
    fn follow_up(&self, command: Command) {
        match command {
            Command::Subscribe { topics } => {
                for topic in topics {
                    if topic == Topic::Logs {
                        self.daemon.logger.subscribe(self.id);
                        continue;
                    }
                    // Subscribed and sent the current state under the
                    // settings lock, so a reload's event can't come first.
                    let settings = lock(&self.daemon.settings);
                    self.daemon.hub.subscribe(self.id, topic);
                    match topic {
                        Topic::Features => self.send(Event::Features { features: settings.features.clone() }),
                        Topic::Theme => self.send(Event::Theme(settings.theme.clone())),
                        // Without the feature there is nothing to send until
                        // a reload turns it on.
                        Topic::AiUsage => {
                            if let Some(usage) = &settings.ai_usage {
                                self.send(usage.event());
                            }
                        }
                        // Sent once the compositor has said what it shows.
                        Topic::Compositor => {
                            if let Some(compositor) = &self.daemon.compositor {
                                compositor.subscribe(self.id);
                            }
                        }
                        Topic::Lock => self.daemon.session_lock.send_state(&self.daemon.hub, self.id),
                        // Only ever sent as `nova toggle` asks.
                        Topic::Logs | Topic::Toggle => {}
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
            | Command::ReportClaudeUsage { .. }
            | Command::FocusWorkspace { .. }
            | Command::StepWorkspace { .. }
            | Command::Toggle { .. }
            | Command::Lock
            | Command::ReportLocked
            | Command::ReportUnlocked => {}
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

/// Writes a client's queue in order until it closes. A write that fails or
/// stalls past [`WRITE_TIMEOUT`] hangs up on the client: the reader side ends
/// too, so the connection unregisters, and dropping the queue answers any
/// `flush` waiting on it.
fn write_all_queued(mut writer: UnixStream, outgoing: mpsc::Receiver<Outgoing>, logger: &crate::logger::Logger) {
    for message in outgoing {
        match message {
            Outgoing::Line(line) => {
                if let Err(e) = writer.write_all(line.as_bytes()) {
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
                        logger.warn(
                            LOG_SOURCE,
                            &format!("a client read nothing for {} s, disconnecting it", WRITE_TIMEOUT.as_secs_f32()),
                        );
                    }
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
            Outgoing::Flushed(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// The `id` of a request that didn't parse, so the error can be answered.
fn request_id(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line).ok()?.get("id")?.as_u64()
}
