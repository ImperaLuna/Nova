//! Fans events out to connected clients by topic. Each client has one ordered
//! queue, drained by its own writer thread, so a slow client never blocks
//! novad and replies and events reach it in the order they were sent.
//!
//! An event is anything [`Published`]: the hub reads only its topic, so a
//! feature's events and topics are its own types.

use nova_base::lock;
use nova_carrier::{Published, Topic, encode};
use serde::Serialize;
use std::collections::HashSet;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

/// Most messages a client may have waiting, and most bytes they may add up
/// to. A client that stops reading fills its queue and is disconnected, so
/// it can't grow novad's memory.
pub const MAX_QUEUED_MESSAGES: usize = 4096;
pub const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;

/// What a client's writer thread sends, in order.
pub enum Outgoing {
    Line(String),
    /// Answered once every line queued before it is written.
    Flushed(SyncSender<()>),
}

/// One client's outgoing queue. Pushing never blocks.
#[derive(Clone)]
pub struct ClientQueue {
    sender: SyncSender<Outgoing>,
    stream: Arc<UnixStream>,
    /// The bytes of the lines queued and not yet written.
    bytes: Arc<AtomicUsize>,
}

impl ClientQueue {
    /// A queue for `stream`, and the end its writer thread drains.
    pub fn new(stream: &UnixStream) -> std::io::Result<(Self, Receiver<Outgoing>)> {
        let (sender, receiver) = mpsc::sync_channel(MAX_QUEUED_MESSAGES);
        let bytes = Arc::default();
        Ok((Self { sender, stream: Arc::new(stream.try_clone()?), bytes }, receiver))
    }

    /// Queues `message`. False if the client is gone, or was just
    /// disconnected for letting its queue fill up.
    pub fn push(&self, message: Outgoing) -> bool {
        let length = match &message {
            Outgoing::Line(line) => line.len(),
            Outgoing::Flushed(_) => 0,
        };
        if self.bytes.fetch_add(length, Ordering::Relaxed) + length > MAX_QUEUED_BYTES {
            self.bytes.fetch_sub(length, Ordering::Relaxed);
            return self.hang_up();
        }
        match self.sender.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.bytes.fetch_sub(length, Ordering::Relaxed);
                self.hang_up()
            }
            Err(TrySendError::Disconnected(_)) => {
                self.bytes.fetch_sub(length, Ordering::Relaxed);
                false
            }
        }
    }

    /// What the writer tells, as it takes each line off the queue. Not a
    /// queue itself, so the writer doesn't hold the queue open.
    pub fn written(&self) -> Written {
        Written(Arc::clone(&self.bytes))
    }

    /// Ends both the writer's and the reader's side, so the connection
    /// closes and unregisters itself. Always false, for `push`.
    fn hang_up(&self) -> bool {
        let _ = self.stream.shutdown(Shutdown::Both);
        false
    }
}

/// Counts the lines a client's writer takes off its queue (`ClientQueue::written`).
pub struct Written(Arc<AtomicUsize>);

impl Written {
    /// The writer took a line of `length` bytes off the queue.
    pub fn line(&self, length: usize) {
        self.0.fetch_sub(length, Ordering::Relaxed);
    }
}

pub type ClientId = u64;

#[derive(Default)]
pub struct Hub {
    clients: Mutex<Clients>,
}

#[derive(Default)]
struct Clients {
    next_id: ClientId,
    list: Vec<Client>,
}

struct Client {
    id: ClientId,
    topics: HashSet<Topic>,
    queue: ClientQueue,
}

impl Hub {
    pub fn register(&self, queue: ClientQueue) -> ClientId {
        let mut clients = lock(&self.clients);
        clients.next_id += 1;
        let id = clients.next_id;
        clients.list.push(Client { id, topics: HashSet::new(), queue });
        id
    }

    pub fn unregister(&self, id: ClientId) {
        lock(&self.clients).list.retain(|client| client.id != id);
    }

    pub fn subscribe(&self, id: ClientId, topic: impl Into<Topic>) {
        let topic = topic.into();
        if let Some(client) = lock(&self.clients).list.iter_mut().find(|client| client.id == id) {
            client.topics.insert(topic);
        }
    }

    /// Sends `event` to every client subscribed to its topic. Returns how
    /// many it was queued for.
    pub fn publish(&self, event: &impl Published) -> usize {
        let Some(topic) = event.topic() else { return 0 };
        let Some(line) = Self::encode(event) else { return 0 };
        // A failed push means the client is gone or being disconnected; its
        // connection thread unregisters it, so there is nothing to do here.
        lock(&self.clients)
            .list
            .iter()
            .filter(|client| client.topics.contains(&topic))
            .filter(|client| client.queue.push(Outgoing::Line(line.clone())))
            .count()
    }

    /// Sends `event` to one client, whatever it subscribed to. Returns
    /// whether it was queued: not for a client that's gone.
    pub fn send_to(&self, id: ClientId, event: &impl Serialize) -> bool {
        Self::encode(event).is_some_and(|line| self.send_line_to(id, line))
    }

    /// `event` as the line [`Hub::send_line_to`] sends, so a big event can
    /// be encoded before taking a lock it's sent under. `None` when it
    /// can't be (reported).
    pub fn encode(event: &impl Serialize) -> Option<String> {
        // Not logged through the logger: it publishes through this hub.
        encode(event).map_err(|e| eprintln!("novad: can't encode an event: {e}")).ok()
    }

    /// Sends a line from [`Hub::encode`] to one client, like
    /// [`Hub::send_to`].
    pub fn send_line_to(&self, id: ClientId, line: String) -> bool {
        lock(&self.clients)
            .list
            .iter()
            .find(|client| client.id == id)
            .is_some_and(|client| client.queue.push(Outgoing::Line(line)))
    }
}

/// The next event in a client's queue, as JSON, waiting up to `wait`; flush
/// markers are skipped. For tests that read a service's events the way a
/// connection would.
#[cfg(any(test, feature = "test-support"))]
pub fn next_event(queue: &Receiver<Outgoing>, wait: std::time::Duration) -> serde_json::Value {
    let deadline = std::time::Instant::now() + wait;
    loop {
        match queue.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok(sent) => {
                if let Some(event) = sent_event(sent) {
                    return event;
                }
            }
            Err(e) => panic!("no event in time: {e}"),
        }
    }
}

/// What a client was sent, as a JSON event, or None for a flush marker.
#[cfg(any(test, feature = "test-support"))]
fn sent_event(sent: Outgoing) -> Option<serde_json::Value> {
    match sent {
        Outgoing::Line(line) => Some(serde_json::from_str(&line).unwrap()),
        Outgoing::Flushed(_) => None,
    }
}

/// The data of the next event called `name` in a client's queue, skipping
/// others, like [`next_event`].
#[cfg(any(test, feature = "test-support"))]
pub fn next_named(queue: &Receiver<Outgoing>, name: &str, wait: std::time::Duration) -> serde_json::Value {
    named_within(queue, name, wait).unwrap_or_else(|| panic!("no {name} event in time"))
}

/// The data of the next event called `name` within `wait`, skipping others,
/// or None if none comes.
#[cfg(any(test, feature = "test-support"))]
pub fn named_within(queue: &Receiver<Outgoing>, name: &str, wait: std::time::Duration) -> Option<serde_json::Value> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        let left = deadline.checked_duration_since(std::time::Instant::now())?;
        match queue.recv_timeout(left) {
            Ok(sent) => {
                if let Some(event) = sent_event(sent).filter(|event| event["event"] == name) {
                    return Some(event["data"].clone());
                }
            }
            Err(_) => return None,
        }
    }
}

/// The names of the events a client's queue holds now, in order, taking
/// them all off: for checking several events didn't come, in one pass.
#[cfg(any(test, feature = "test-support"))]
pub fn queued_names(queue: &Receiver<Outgoing>) -> Vec<String> {
    queue
        .try_iter()
        .filter_map(sent_event)
        .map(|event| event["event"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_holds_at_most_its_bytes() {
        let (_client, ours) = UnixStream::pair().unwrap();
        let (queue, outgoing) = ClientQueue::new(&ours).unwrap();
        let quarter = "x".repeat(MAX_QUEUED_BYTES / 4);
        for _ in 0..4 {
            assert!(queue.push(Outgoing::Line(quarter.clone())));
        }
        // What the writer takes off makes room again.
        let Ok(Outgoing::Line(line)) = outgoing.try_recv() else { panic!("a line") };
        queue.written().line(line.len());
        assert!(queue.push(Outgoing::Line(quarter)));
        assert!(!queue.push(Outgoing::Line("x".into())), "past the bytes the client is cut off");
    }
}
