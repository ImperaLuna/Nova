//! novad's socket: accepts clients, answers their requests and feeds them
//! the events they subscribed to, without knowing any of them.
//!
//! What a request means is the app's: it builds a [`Router`], where each
//! message type it serves is one route, and hands it over in its
//! [`Service`]. A connection reads one line at a time, checks the envelope,
//! and lets the route its `cmd` names answer ([`Answer`]) and then act
//! ([`Router::route`]).

mod router;
mod server;

pub use router::{Answer, Caller, Router, done, value};
pub use server::{MAX_REQUEST_BYTES, Service, WRITE_TIMEOUT, bind, serve};
