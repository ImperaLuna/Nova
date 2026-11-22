//! The socket: accepts clients, reads their request lines and writes their
//! queues. Each client gets a thread that reads and one that writes, so a
//! slow client never holds up another or novad.

use crate::router::{Caller, Router};
use nova_carrier::{Outcome, Response, read_request};
use nova_hub::{ClientId, ClientQueue, Hub, Outgoing, Written};
use nova_log::Logger;
use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Any local process can connect: longer request lines close the connection.
pub const MAX_REQUEST_BYTES: u64 = nova_carrier::envelope::REQUEST_MAX_BYTES;
/// How long a write may make no progress before the client counts as stalled
/// and is disconnected. Only a client that stops reading with its socket
/// buffer full gets there; the UI reconnects on its own.
pub const WRITE_TIMEOUT: Duration =
    if cfg!(any(test, feature = "test-support")) { Duration::from_millis(300) } else { Duration::from_secs(10) };
const SOCKET_DIR_MODE: u32 = 0o700;

const LOG_SOURCE: &str = "server";

/// The app behind the socket.
pub trait Service: Send + Sync + Sized + 'static {
    /// What every client is sent first.
    type Hello: Serialize;

    fn hub(&self) -> &Hub;
    fn logger(&self) -> &Arc<Logger>;
    /// The routes requests go to.
    fn router(&self) -> &Router<Self>;
    fn hello(&self) -> Self::Hello;
    /// The client `client` hung up; the hub has forgotten it already.
    fn client_gone(&self, client: ClientId);
}

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

/// Accepts clients until `stopped` is set; the connection after that (one
/// made to wake it) ends the loop.
pub fn serve<S: Service>(listener: UnixListener, service: Arc<S>, stopped: &AtomicBool) {
    for stream in listener.incoming() {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        match stream {
            Ok(stream) => {
                let service = Arc::clone(&service);
                std::thread::spawn(move || {
                    if let Err(e) = Connection::open(stream, &*service).and_then(|c| c.run()) {
                        service.logger().warn(LOG_SOURCE, &format!("client error: {e}"));
                    }
                });
            }
            Err(e) => service.logger().warn(LOG_SOURCE, &format!("accept failed: {e}")),
        }
    }
}

struct Connection<'a, S: Service> {
    service: &'a S,
    caller: Caller,
    reader: BufReader<UnixStream>,
}

impl<'a, S: Service> Connection<'a, S> {
    fn open(stream: UnixStream, service: &'a S) -> std::io::Result<Self> {
        let writer = stream.try_clone()?;
        writer.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let (queue, outgoing) = ClientQueue::new(&stream)?;
        let logger = Arc::clone(service.logger());
        let written = queue.written();
        std::thread::spawn(move || write_all_queued(writer, outgoing, &written, &logger));
        let id = service.hub().register(queue.clone());
        let caller = Caller::new(id, peer_pid(&stream), queue, Arc::clone(service.logger()));
        let connection = Self { service, caller, reader: BufReader::new(stream) };
        connection.caller.send(service.hello());
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
        let handled = read_request(line).and_then(|raw| self.service.router().handle(self.service, &self.caller, raw));
        if let Err(e) = handled {
            self.service.logger().warn(LOG_SOURCE, &format!("ignoring bad request: {e}"));
            if let Some(id) = request_id(line) {
                self.caller.send(Response { id, outcome: Outcome::Error(format!("bad request: {e}")) });
            }
        }
    }
}

impl<S: Service> Drop for Connection<'_, S> {
    fn drop(&mut self) {
        self.service.hub().unregister(self.caller.id());
        self.service.client_gone(self.caller.id());
    }
}

/// Writes a client's queue in order until it closes. A write that fails or
/// stalls past [`WRITE_TIMEOUT`] hangs up on the client: the reader side ends
/// too, so the connection unregisters, and dropping the queue answers any
/// `flush` waiting on it.
fn write_all_queued(mut writer: UnixStream, outgoing: mpsc::Receiver<Outgoing>, written: &Written, logger: &Logger) {
    for message in outgoing {
        match message {
            Outgoing::Line(line) => {
                written.line(line.len());
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

/// The pid of the process at the other end of `stream`.
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for the size passed.
    let got = unsafe {
        libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut cred).cast(), &mut len)
    };
    (got == 0).then(|| u32::try_from(cred.pid).ok()).flatten()
}
