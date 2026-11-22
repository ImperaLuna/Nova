//! A stand-in sound server on a test's socket, answering PulseAudio's
//! protocol from a recording (`recorded/*.pulse`: what a real server
//! answered each listing request, one answer per line as `<name> <hex>`).
//! It lists what the recording holds, tells its followers when a test
//! changes that, and can hang up or go away as a real one does.

use crate::server::Listing;
use nova_base::lock;
use pulseaudio::protocol::{
    self, AuthReply, Command, CommandReply, PulseError, SetClientNameReply, SubscriptionEvent,
    SubscriptionEventFacility, SubscriptionEventType,
};
use std::io::{BufReader, Cursor};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The version recordings are written in, and the fake speaks.
const VERSION: u16 = protocol::MAX_VERSION;
/// The request number of what the server sends unasked.
const UNASKED: u32 = u32::MAX;
/// The lines of a recording, in order.
const SERVER: &str = "server";
const SINKS: &str = "sinks";
const SOURCES: &str = "sources";
const SINK_INPUTS: &str = "sink_inputs";
const SOURCE_OUTPUTS: &str = "source_outputs";

/// The listing a recording holds.
pub fn recorded(text: &str) -> Listing {
    fn answer<R: CommandReply>(text: &str, name: &str) -> R {
        let line = text.lines().find_map(|line| line.strip_prefix(name)?.strip_prefix(' '));
        let hex = line.unwrap_or_else(|| panic!("the recording has no {name} line"));
        let bytes: Vec<u8> =
            (0..hex.len()).step_by(2).map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap()).collect();
        protocol::read_reply_message::<R>(&mut Cursor::new(bytes), VERSION).unwrap().1
    }
    Listing {
        server: answer(text, SERVER),
        sinks: answer(text, SINKS),
        sources: answer(text, SOURCES),
        sink_inputs: answer(text, SINK_INPUTS),
        source_outputs: answer(text, SOURCE_OUTPUTS),
    }
}

/// `listing` as a recording's text.
pub fn recording(listing: &Listing) -> String {
    fn line<R: CommandReply>(name: &str, reply: &R) -> String {
        let mut bytes = Vec::new();
        protocol::write_reply_message(&mut bytes, 0, reply, VERSION).unwrap();
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{name} {hex}\n")
    }
    [
        line(SERVER, &listing.server),
        line(SINKS, &listing.sinks),
        line(SOURCES, &listing.sources),
        line(SINK_INPUTS, &listing.sink_inputs),
        line(SOURCE_OUTPUTS, &listing.source_outputs),
    ]
    .concat()
}

pub struct FakePulse {
    socket: PathBuf,
    state: Arc<Mutex<Fake>>,
}

struct Fake {
    listing: Listing,
    /// Every connection, to hang up on.
    connections: Vec<UnixStream>,
    /// The ones that asked to be told of changes.
    followers: Vec<UnixStream>,
    /// How many listings were asked for (their first request).
    listings: usize,
    /// Listings are answered with an error.
    refusing: bool,
    gone: bool,
}

impl FakePulse {
    /// Serves `listing` on a new socket at `socket`.
    pub fn start(socket: &Path, listing: Listing) -> Self {
        let listener = UnixListener::bind(socket).unwrap();
        let state = Arc::new(Mutex::new(Fake {
            listing,
            connections: Vec::new(),
            followers: Vec::new(),
            listings: 0,
            refusing: false,
            gone: false,
        }));
        let serving = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut fake = lock(&serving);
                if fake.gone {
                    return;
                }
                fake.connections.push(stream.try_clone().unwrap());
                drop(fake);
                let serving = Arc::clone(&serving);
                std::thread::spawn(move || serve(stream, &serving));
            }
        });
        Self { socket: socket.to_owned(), state }
    }

    /// Changes what it lists and tells its followers, as the real server
    /// does whoever made the change.
    pub fn change(&self, change: impl FnOnce(&mut Listing)) {
        let mut fake = lock(&self.state);
        change(&mut fake.listing);
        fake.tell_followers();
    }

    /// Tells its followers something changed, `times` over, as a dragged
    /// slider does.
    pub fn say_changed(&self, times: usize) {
        let mut fake = lock(&self.state);
        for _ in 0..times {
            fake.tell_followers();
        }
    }

    /// Answers every listing with an error from here on, as a server
    /// that takes connections but can't list does.
    pub fn refuse_listings(&self) {
        lock(&self.state).refusing = true;
    }

    /// How many listings were asked for so far.
    pub fn listings(&self) -> usize {
        lock(&self.state).listings
    }

    /// How many connections follow its changes now.
    pub fn followers(&self) -> usize {
        lock(&self.state).followers.len()
    }

    /// Closes every connection, as a server that restarts does; new ones
    /// are still taken.
    pub fn hang_up(&self) {
        let mut fake = lock(&self.state);
        for connection in fake.connections.drain(..) {
            let _ = connection.shutdown(Shutdown::Both);
        }
        fake.followers.clear();
    }
}

/// Goes away: its socket with it, and every connection.
impl Drop for FakePulse {
    fn drop(&mut self) {
        lock(&self.state).gone = true;
        self.hang_up();
        // Ends the wait for the next connection.
        let _ = UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Fake {
    fn tell_followers(&mut self) {
        let event = Command::SubscribeEvent(SubscriptionEvent {
            event_facility: SubscriptionEventFacility::Server,
            event_type: SubscriptionEventType::Changed,
            index: None,
        });
        self.followers
            .retain_mut(|follower| protocol::write_command_message(follower, UNASKED, &event, VERSION).is_ok());
    }
}

/// Answers one connection's requests until it closes.
fn serve(stream: UnixStream, state: &Mutex<Fake>) {
    let mut reader = BufReader::new(stream);
    while let Ok((request, command)) = protocol::read_command_message(&mut reader, VERSION) {
        let mut fake = lock(state);
        let to = reader.get_mut();
        let sent = match command {
            Command::Auth(_) => reply(to, request, &AuthReply { version: VERSION, use_memfd: false, use_shm: false }),
            Command::SetClientName(_) => reply(to, request, &SetClientNameReply { client_id: 1 }),
            Command::Subscribe(_) => {
                fake.followers.push(to.try_clone().unwrap());
                protocol::write_ack_message(to, request)
            }
            Command::GetServerInfo => {
                fake.listings += 1;
                match fake.refusing {
                    true => protocol::write_error(to, request, &PulseError::Internal),
                    false => reply(to, request, &fake.listing.server),
                }
            }
            Command::GetSinkInfoList => reply(to, request, &fake.listing.sinks),
            Command::GetSourceInfoList => reply(to, request, &fake.listing.sources),
            Command::GetSinkInputInfoList => reply(to, request, &fake.listing.sink_inputs),
            Command::GetSourceOutputInfoList => reply(to, request, &fake.listing.source_outputs),
            other => panic!("the fake server doesn't answer {other:?}"),
        };
        if sent.is_err() {
            return;
        }
    }
}

fn reply<R: CommandReply>(to: &mut UnixStream, request: u32, reply: &R) -> Result<(), protocol::ProtocolError> {
    protocol::write_reply_message(to, request, reply, VERSION)
}
