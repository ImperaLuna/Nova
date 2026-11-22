//! What carries Nova's messages between novad and its clients without
//! knowing what they say. A feature's messages are its own types; this crate
//! only frames them and names them:
//!
//! - [`envelope`]: one JSON object per line, with the protocol version, the
//!   request's `id` and `cmd`/`args`, and the answer's `result`/`error`.
//! - [`names`]: the wire names a message type accepts, read from serde, so a
//!   router or a generated file needs no list kept by hand.
//! - [`topic`]: the [`Topic`] a client subscribes to and the [`Published`]
//!   trait every event type implements, so the hub sends a feature's events
//!   without naming them.
//! - [`log`]: novad's own log feed, the one event the framework sends itself.
//!
//! Nothing here depends on the daemon, so a client (the `nova` command, a
//! Rust UI) decodes with the same code novad encodes with.

pub mod envelope;
pub mod log;
pub mod names;
pub mod topic;

pub use envelope::{
    Envelope, Outcome, PROTOCOL_VERSION, RawRequest, Request, Response, decode_request, encode, read_request,
};
pub use topic::{Published, Topic};
