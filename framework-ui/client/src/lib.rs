//! The UI's connection to novad, over the one socket every client uses
//! (`nova-carrier` frames the lines, each feature's api crate types them).
//!
//! `nova-ui` connects once at its start ([`Daemon::connect`]). A feature's
//! ui crate then asks for what it shows, with its api crate's types:
//!
//! ```ignore
//! use nova_sound_api::{Event, Sound, TOPIC};
//! use nova_ui_client::{Daemon, Live};
//!
//! // The latest state of a topic, as an entity a view observes.
//! let sound: Entity<Live<Sound>> = Live::follow(TOPIC, cx, |Event::Sound(sound)| Some(sound));
//! cx.observe(&sound, |_, _, cx| cx.notify()).detach();
//! let outputs = sound.read(cx).get().map(|sound| &sound.outputs);
//!
//! // Events that are no state (a toggle, a saved screenshot).
//! let toggles = Daemon::on_event(nova_bar_api::TOPIC, cx, |event: nova_bar_api::Event, cx| { /* ... */ });
//!
//! // Commands: with the result's type, or only sent (a refusal is logged).
//! let status = Daemon::call::<nova_daemon_info::Status>(Command::Status, cx);
//! Daemon::send(nova_workspaces_api::Command::FocusWorkspace { id: 3 }, cx);
//! ```
//!
//! The connection comes back by itself when novad restarts: the topics are
//! subscribed to again, novad answers each with its state, and until then a
//! [`Live`] keeps showing the last one. [`Daemon::connection`] says whether
//! novad is there.
//!
//! - [`link`]: the socket itself, on its own thread, without GPUI.
//! - [`daemon`]: the link brought to GPUI's thread, and [`Live`].
//! - [`fake`]: a novad that replays recorded events.
//! - [`watch`]: something novad does only while asked, which views on
//!   several screens share ([`Watch`]).

pub mod daemon;
pub mod fake;
pub mod link;
mod log;
mod routes;
#[cfg(test)]
mod tests;
pub mod watch;

pub use daemon::{Connection, Daemon, Live, Status};
pub use link::Refusal;
pub use watch::Watch;
