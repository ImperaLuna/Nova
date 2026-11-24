mod changes;
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
use nova_sound_api::{Command, Event, Sound as Reading};
use nova_volume_osd_api::VolumeKey;
use pulseaudio::protocol::Command as Request;
pub use server::Sources;
use server::{Failed, Link, Listing};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

pub const FEATURE_ID: &str = "sound";
const LOG_SOURCE: &str = "sound";

const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);

const NO_SERVER: &str = "there is no sound server";
const SERVER_CAME_BACK: &str = "the sound server restarted, look at the sound again";
const CLOSED: &str = "the connection closed";

pub struct Sound {
    sources: Sources,
    state: Mutex<State>,
    asking: Mutex<Option<(Link, u64)>>,
    pressing: Mutex<()>,
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
}

#[derive(Default)]
struct State {
    stopped: bool,
    reading: Reading,
    listing: Option<(Listing, u64)>,
    connection: u64,
    changed: bool,
    gone: u64,
    waiting: Option<UnixStream>,
}

impl Sound {
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let sound = Arc::new(Self {
            sources,
            state: Mutex::new(State::default()),
            asking: Mutex::new(None),
            pressing: Mutex::new(()),
            wake: Condvar::new(),
            hub,
            logger,
        });
        let follower = Arc::clone(&sound);
        std::thread::spawn(move || follower.run());
        sound
    }

    pub fn stop(&self) {
        let mut state = lock(&self.state);
        state.stopped = true;
        state.listing = None;
        end_wait(&mut state);
        drop(state);
        self.wake.notify_all();
    }

    pub fn send_state(&self, client: ClientId) {
        let state = lock(&self.state);
        self.hub.send_to(client, &Event::Sound(state.reading.clone()));
    }

    pub fn change(&self, command: &Command) -> Result<(), String> {
        self.ask(|listing| changes::requests(listing, command))
    }

    pub fn press(&self, key: VolumeKey) -> Result<(), String> {
        let _pressing = lock(&self.pressing);
        let (requests, listed_by) = match &lock(&self.state).listing {
            Some((listing, connection)) => (changes::press(listing, key)?, *connection),
            None => return Err(NO_SERVER.into()),
        };
        self.send(&requests, listed_by)?;
        if let Some((listing, connection)) = lock(&self.state).listing.as_mut()
            && *connection == listed_by
        {
            changes::apply(listing, &requests);
        }
        Ok(())
    }

    fn ask(&self, requests: impl FnOnce(&Listing) -> Result<Vec<Request>, String>) -> Result<(), String> {
        let (requests, listed_by) = match &lock(&self.state).listing {
            Some((listing, connection)) => (requests(listing)?, *connection),
            None => return Err(NO_SERVER.into()),
        };
        self.send(&requests, listed_by)
    }

    fn send(&self, requests: &[Request], listed_by: u64) -> Result<(), String> {
        let mut asking = lock(&self.asking);
        let Some((link, connection)) = asking.as_mut() else {
            return Err(NO_SERVER.into());
        };
        let connection = *connection;
        if connection != listed_by {
            return Err(SERVER_CAME_BACK.into());
        }
        for request in requests {
            if lock(&self.state).stopped {
                return Err(NO_SERVER.into());
            }
            match link.tell(request) {
                Ok(()) => {}
                Err(Failed::Refused(reason)) => return Err(reason),
                Err(Failed::Broken(reason)) => {
                    *asking = None;
                    drop(asking);
                    let mut state = lock(&self.state);
                    state.gone = state.gone.max(connection);
                    drop(state);
                    self.wake.notify_all();
                    return Err(format!("the sound server went away: {reason}"));
                }
            }
        }
        Ok(())
    }

    fn run(self: Arc<Self>) {
        let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
        let mut said: Option<String> = None;
        loop {
            if lock(&self.state).stopped {
                return;
            }
            let reason = match self.connect() {
                Ok(connection) => self.follow(connection, || {
                    retry.reset();
                    said = None;
                }),
                Err(reason) => reason,
            };
            if !self.tell(None) {
                return;
            }
            if said.as_deref() != Some(reason.as_str()) {
                self.logger.warn(LOG_SOURCE, &format!("no sound server: {reason}"));
                said = Some(reason);
            }
            wait_out(retry.next());
        }
    }

    fn connect(self: &Arc<Self>) -> Result<u64, String> {
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
        state.changed = true;
        drop(state);
        *lock(&self.asking) = Some((asking, connection));
        let sound = Arc::clone(self);
        std::thread::spawn(move || sound.wait_for_changes(waiting, connection));
        Ok(connection)
    }

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

    fn follow(&self, connection: u64, answered: impl FnMut()) -> String {
        let reason = self.follow_until_gone(connection, answered);
        let mut asking = lock(&self.asking);
        if asking.as_ref().is_some_and(|(_, count)| *count == connection) {
            *asking = None;
        }
        drop(asking);
        end_wait(&mut lock(&self.state));
        reason
    }

    fn follow_until_gone(&self, connection: u64, mut answered: impl FnMut()) -> String {
        let mut named = false;
        loop {
            let mut state = lock(&self.state);
            while !state.stopped && !state.changed && state.gone < connection {
                state = self.wake.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            if state.stopped || state.gone >= connection {
                return CLOSED.into();
            }
            state.changed = false;
            drop(state);
            let listing = match lock(&self.asking).as_mut() {
                Some((asking, count)) if *count == connection => asking.listing(),
                _ => Err(CLOSED.into()),
            };
            let listing = match listing {
                Ok(listing) => listing,
                Err(reason) => return reason,
            };
            if !named {
                let server = listing.server.server_name.as_deref().map(|name| name.to_string_lossy());
                self.logger.info(LOG_SOURCE, &format!("following {}", server.as_deref().unwrap_or("the server")));
                named = true;
                answered();
            }
            let arrived = self.arrived_bluetooth(&listing, connection);
            if !self.tell(Some((listing, connection))) {
                return "stopped".into();
            }
            self.use_arrived(&arrived, connection);
        }
    }

    fn arrived_bluetooth(&self, listing: &Listing, connection: u64) -> Vec<Request> {
        let state = lock(&self.state);
        let before = state.listing.as_ref().filter(|(_, listed_by)| *listed_by == connection);
        changes::arrived_bluetooth(before.map(|(listing, _)| listing), listing)
    }

    fn use_arrived(&self, requests: &[Request], connection: u64) {
        for request in requests {
            let (what, name) = match request {
                Request::SetDefaultSink(name) => ("sound", name),
                Request::SetDefaultSource(name) => ("the mic", name),
                _ => continue,
            };
            self.logger.info(LOG_SOURCE, &format!("switching {what} to {}", name.to_string_lossy()));
        }
        if requests.is_empty() {
            return;
        }
        if let Err(reason) = self.send(requests, connection) {
            self.logger.warn(LOG_SOURCE, &format!("can't switch to the Bluetooth device: {reason}"));
        }
    }

    fn tell(&self, listing: Option<(Listing, u64)>) -> bool {
        let reading = listing.as_ref().map(|(listing, _)| reading::sound(listing)).unwrap_or_default();
        let mut state = lock(&self.state);
        if state.stopped {
            return false;
        }
        state.listing = listing;
        if state.reading != reading {
            state.reading = reading;
            self.hub.publish(&Event::Sound(state.reading.clone()));
        }
        true
    }
}

fn end_wait(state: &mut State) {
    if let Some(socket) = state.waiting.take() {
        let _ = socket.shutdown(Shutdown::Both);
    }
}
