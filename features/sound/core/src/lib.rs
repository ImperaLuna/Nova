//! Sound (feature `sound`, its messages in `nova-sound-api`): every output
//! and input, the apps playing, their volume and mute, which devices are in
//! use and who records from the mic.
//!
//! It follows PipeWire through the PulseAudio socket `pipewire-pulse`
//! serves ([`server`]), with two connections: one only waits for the
//! server to say something changed, the other asks for the whole listing
//! again each time, which [`reading`] turns into the feature's
//! [`Sound`](Reading). Changes that come while a listing is read cost one
//! more listing, however many they are. What clients get is published only
//! when it differs from the last one.
//!
//! Without the server (no `pipewire-pulse`, or it went away) the feature
//! says so (`available: false`) and tries again with a doubling wait, since
//! a server that isn't there sends nothing to wait on.
//!
//! novad runs it as a module (its wiring adapts [`Sound`] to novad's
//! `Module` trait).

#[cfg(test)]
mod fake_pulse;
mod reading;
mod server;
#[cfg(test)]
mod tests;

use nova_base::backoff::{Backoff, wait_out};
use nova_base::lock;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_sound_api::{Event, Sound as Reading};
use server::Link;
pub use server::Sources;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

pub const FEATURE_ID: &str = "sound";
const LOG_SOURCE: &str = "sound";

/// The waits between tries while the server isn't there.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);

pub struct Sound {
    sources: Sources,
    state: Mutex<State>,
    /// Wakes the follower: something changed, the server went, or stop.
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
}

#[derive(Default)]
struct State {
    stopped: bool,
    /// What clients were last told, and a new one is.
    reading: Reading,
    /// The connections now, counted so one that already went can't end
    /// the next.
    connection: u64,
    /// The server changed something since the last listing was asked for.
    changed: bool,
    /// The newest connection that went, by its count: never lowered, so
    /// an older one going late can't hide it.
    gone: u64,
    /// The socket that waits for changes, to end its wait with.
    waiting: Option<UnixStream>,
}

impl Sound {
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let sound = Arc::new(Self { sources, state: Mutex::new(State::default()), wake: Condvar::new(), hub, logger });
        let follower = Arc::clone(&sound);
        std::thread::spawn(move || follower.run());
        sound
    }

    /// Ends following; nothing is published after this returns.
    pub fn stop(&self) {
        let mut state = lock(&self.state);
        state.stopped = true;
        end_wait(&mut state);
        drop(state);
        self.wake.notify_all();
    }

    /// Sends `client` the sound as it is now.
    pub fn send_state(&self, client: ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(client, &Event::Sound(state.reading.clone()));
    }

    /// Connects, follows until the connection goes, and again, waiting
    /// longer each time the server isn't there.
    fn run(self: Arc<Self>) {
        let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
        // The last reason said, so a server that stays away is logged once.
        let mut said: Option<String> = None;
        loop {
            if lock(&self.state).stopped {
                return;
            }
            let reason = match self.connect() {
                // A server that answered is one worth trying again soon,
                // and news when it goes; one that connects but never
                // lists keeps the longer waits and is said once.
                Ok((asking, connection)) => self.follow(asking, connection, || {
                    retry.reset();
                    said = None;
                }),
                Err(reason) => reason,
            };
            if !self.tell(Reading::default()) {
                return;
            }
            if said.as_deref() != Some(reason.as_str()) {
                self.logger.warn(LOG_SOURCE, &format!("no sound server: {reason}"));
                said = Some(reason);
            }
            wait_out(retry.next());
        }
    }

    /// Both connections: the one that asks, and the one that waits for
    /// changes, on its own thread from here. Returns the first and this
    /// connection's count.
    fn connect(self: &Arc<Self>) -> Result<(Link, u64), String> {
        let asking = Link::connect(&self.sources)?;
        let mut waiting = Link::connect(&self.sources)?;
        waiting.follow()?;
        let socket = waiting.socket().map_err(|e| format!("can't share the socket: {e}"))?;
        let mut state = lock(&self.state);
        if state.stopped {
            return Err("stopped".into());
        }
        state.connection += 1;
        let connection = state.connection;
        state.waiting = Some(socket);
        // Following began before this listing, so nothing is missed.
        state.changed = true;
        drop(state);
        let sound = Arc::clone(self);
        std::thread::spawn(move || sound.wait_for_changes(waiting, connection));
        Ok((asking, connection))
    }

    /// Passes each change on to the follower, until the connection goes.
    fn wait_for_changes(&self, mut waiting: Link, connection: u64) {
        loop {
            let change = waiting.next_change();
            let mut state = lock(&self.state);
            match change {
                Ok(()) => state.changed = true,
                Err(_) => state.gone = state.gone.max(connection),
            }
            drop(state);
            self.wake.notify_all();
            if change.is_err() {
                return;
            }
        }
    }

    /// Reads the listing whenever something changed and publishes what
    /// differs, until `connection` goes or the feature stops. Returns why.
    /// `answered` runs once, after the first listing.
    fn follow(&self, mut asking: Link, connection: u64, mut answered: impl FnMut()) -> String {
        let mut named = false;
        loop {
            let mut state = lock(&self.state);
            while !state.stopped && !state.changed && state.gone < connection {
                state = self.wake.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            if state.stopped || state.gone >= connection {
                end_wait(&mut state);
                return "the connection closed".into();
            }
            state.changed = false;
            drop(state);
            let listing = match asking.listing() {
                Ok(listing) => listing,
                Err(reason) => {
                    end_wait(&mut lock(&self.state));
                    return reason;
                }
            };
            if !named {
                let server = listing.server.server_name.as_deref().map(|name| name.to_string_lossy());
                self.logger.info(LOG_SOURCE, &format!("following {}", server.as_deref().unwrap_or("the server")));
                named = true;
                answered();
            }
            if !self.tell(reading::sound(&listing)) {
                return "stopped".into();
            }
        }
    }

    /// Publishes `reading` when it differs from the last one. False once
    /// the feature stopped: nothing is published then.
    fn tell(&self, reading: Reading) -> bool {
        let mut state = lock(&self.state);
        if state.stopped {
            return false;
        }
        if state.reading != reading {
            state.reading = reading;
            // Published under the state lock, so readings reach clients in
            // order and a `stop` can't be overtaken by one.
            self.hub.publish(&Event::Sound(state.reading.clone()));
        }
        true
    }
}

/// Ends the wait for changes: its thread sees the socket close and leaves.
fn end_wait(state: &mut State) {
    if let Some(socket) = state.waiting.take() {
        // Already closed by the server is fine.
        let _ = socket.shutdown(Shutdown::Both);
    }
}
