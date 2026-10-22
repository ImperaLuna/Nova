//! Fans events out to connected clients by topic. Each client has one ordered
//! queue, drained by its own writer thread, so a slow client never blocks
//! novad and replies and events reach it in the order they were sent.

use crate::lock;
use nova_protocol::{Event, Topic, encode};
use std::collections::HashSet;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

/// Most messages a client may have waiting. A client that stops reading
/// fills its queue and is disconnected, so it can't grow novad's memory.
pub const MAX_QUEUED_MESSAGES: usize = 4096;

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
}

impl ClientQueue {
    /// A queue for `stream`, and the end its writer thread drains.
    pub fn new(stream: &UnixStream) -> std::io::Result<(Self, Receiver<Outgoing>)> {
        let (sender, receiver) = mpsc::sync_channel(MAX_QUEUED_MESSAGES);
        Ok((Self { sender, stream: Arc::new(stream.try_clone()?) }, receiver))
    }

    /// Queues `message`. False if the client is gone, or was just
    /// disconnected for letting its queue fill up.
    pub fn push(&self, message: Outgoing) -> bool {
        match self.sender.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                // Ends both the writer's and the reader's side, so the
                // connection closes and unregisters itself.
                let _ = self.stream.shutdown(Shutdown::Both);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
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

    pub fn subscribe(&self, id: ClientId, topic: Topic) {
        if let Some(client) = lock(&self.clients).list.iter_mut().find(|client| client.id == id) {
            client.topics.insert(topic);
        }
    }

    /// Sends `event` to every client subscribed to its topic.
    pub fn publish(&self, event: &Event) {
        let Some(topic) = event.topic() else { return };
        let Some(line) = encode_event(event) else { return };
        // A failed push means the client is gone or being disconnected; its
        // connection thread unregisters it, so there is nothing to do here.
        for client in lock(&self.clients).list.iter().filter(|client| client.topics.contains(&topic)) {
            client.queue.push(Outgoing::Line(line.clone()));
        }
    }

    /// Sends `event` to one client, whatever it subscribed to.
    pub fn send_to(&self, id: ClientId, event: &Event) {
        let Some(line) = encode_event(event) else { return };
        if let Some(client) = lock(&self.clients).list.iter().find(|client| client.id == id) {
            client.queue.push(Outgoing::Line(line));
        }
    }
}

fn encode_event(event: &Event) -> Option<String> {
    // Not logged through the logger: it publishes through this hub.
    encode(event).map_err(|e| eprintln!("novad: can't encode an event: {e}")).ok()
}
