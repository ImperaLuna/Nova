//! Topics and the events sent on them. An event type is any serializable
//! type, usually an enum tagged `event` with its body in `data`, that says
//! which [`Topic`] it belongs to ([`Published`]). The hub sends it to the
//! clients subscribed to that topic and never looks inside, so each feature
//! owns its events and topics in its own crate.

use serde::Serialize;
use std::fmt;

/// A group of events a client can subscribe to, by its wire name (`"logs"`,
/// `"network"`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Topic(&'static str);

impl Topic {
    /// The topic called `name` on the wire.
    pub const fn named(name: &'static str) -> Self {
        Self(name)
    }

    /// Its wire name.
    pub fn name(self) -> &'static str {
        self.0
    }
}

impl fmt::Debug for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Topic({})", self.0)
    }
}

/// An event: serialized as the body of its line, sent to the clients of its
/// topic.
pub trait Published: Serialize {
    /// The topic a client must subscribe to for this event, `None` for
    /// events every client gets.
    fn topic(&self) -> Option<Topic>;
}

/// An event with one shape, written the way a tagged event enum writes it:
/// `{"event": <name>, "data": <data>}`. For the framework's own events,
/// which have no enum of their own.
#[derive(Serialize)]
pub struct Single<'a, T: Serialize> {
    #[serde(rename = "event")]
    name: &'static str,
    data: &'a T,
    #[serde(skip)]
    topic: Topic,
}

impl<'a, T: Serialize> Single<'a, T> {
    /// The event `name` on `topic`, carrying `data`.
    pub fn new(name: &'static str, topic: Topic, data: &'a T) -> Self {
        Self { name, data, topic }
    }
}

impl<T: Serialize> Published for Single<'_, T> {
    fn topic(&self) -> Option<Topic> {
        Some(self.topic)
    }
}
