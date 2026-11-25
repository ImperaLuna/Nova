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

const VERSION: u16 = protocol::MAX_VERSION;
const UNASKED: u32 = u32::MAX;
const SERVER: &str = "server";
const SINKS: &str = "sinks";
const SOURCES: &str = "sources";
const SINK_INPUTS: &str = "sink_inputs";
const SOURCE_OUTPUTS: &str = "source_outputs";

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
    connections: Vec<UnixStream>,
    followers: Vec<UnixStream>,
    listings: usize,
    refusing: bool,
    gone: bool,
}

impl FakePulse {
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

    pub fn change(&self, change: impl FnOnce(&mut Listing)) {
        let mut fake = lock(&self.state);
        change(&mut fake.listing);
        fake.tell_followers();
    }

    pub fn change_quietly(&self, change: impl FnOnce(&mut Listing)) {
        change(&mut lock(&self.state).listing);
    }

    pub fn listing(&self) -> Listing {
        lock(&self.state).listing.clone()
    }

    pub fn say_changed(&self, times: usize) {
        let mut fake = lock(&self.state);
        for _ in 0..times {
            fake.tell_followers();
        }
    }

    pub fn refuse_listings(&self) {
        lock(&self.state).refusing = true;
    }

    pub fn listings(&self) -> usize {
        lock(&self.state).listings
    }

    pub fn followers(&self) -> usize {
        lock(&self.state).followers.len()
    }

    pub fn hang_up(&self) {
        let mut fake = lock(&self.state);
        for connection in fake.connections.drain(..) {
            let _ = connection.shutdown(Shutdown::Both);
        }
        fake.followers.clear();
    }
}

impl Drop for FakePulse {
    fn drop(&mut self) {
        lock(&self.state).gone = true;
        self.hang_up();
        let _ = UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Fake {
    fn change(&mut self, command: &Command) -> Option<bool> {
        let listing = &mut self.listing;
        let found = match command {
            Command::SetSinkVolume(set) => listing
                .sinks
                .iter_mut()
                .find(|sink| Some(sink.index) == set.device_index)
                .map(|sink| sink.cvolume = set.volume),
            Command::SetSourceVolume(set) => listing
                .sources
                .iter_mut()
                .find(|source| Some(source.index) == set.device_index)
                .map(|source| source.cvolume = set.volume),
            Command::SetSinkInputVolume(set) => listing
                .sink_inputs
                .iter_mut()
                .find(|stream| stream.index == set.index)
                .map(|stream| stream.cvolume = set.volume),
            Command::SetSinkMute(set) => listing
                .sinks
                .iter_mut()
                .find(|sink| Some(sink.index) == set.device_index)
                .map(|sink| sink.muted = set.mute),
            Command::SetSourceMute(set) => listing
                .sources
                .iter_mut()
                .find(|source| Some(source.index) == set.device_index)
                .map(|source| source.muted = set.mute),
            Command::SetSinkInputMute(set) => listing
                .sink_inputs
                .iter_mut()
                .find(|stream| stream.index == set.index)
                .map(|stream| stream.muted = set.mute),
            Command::SetDefaultSink(name) => listing
                .sinks
                .iter()
                .any(|sink| &sink.name == name)
                .then(|| listing.server.default_sink_name = Some(name.clone())),
            Command::SetDefaultSource(name) => listing
                .sources
                .iter()
                .any(|source| &source.name == name)
                .then(|| listing.server.default_source_name = Some(name.clone())),
            _ => return None,
        };
        Some(found.is_some())
    }

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

fn serve(stream: UnixStream, state: &Mutex<Fake>) {
    let mut reader = BufReader::new(stream);
    while let Ok((request, command)) = protocol::read_command_message(&mut reader, VERSION) {
        let mut fake = lock(state);
        let to = reader.get_mut();
        let sent = match command {
            Command::Auth(auth) if auth.cookie.len() != crate::server::COOKIE_LENGTH => {
                protocol::write_error(to, request, &PulseError::Invalid)
            }
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
            other => match fake.change(&other) {
                Some(true) => {
                    fake.tell_followers();
                    protocol::write_ack_message(to, request)
                }
                Some(false) => protocol::write_error(to, request, &PulseError::NoEntity),
                None => panic!("the fake server doesn't answer {other:?}"),
            },
        };
        if sent.is_err() {
            return;
        }
    }
}

fn reply<R: CommandReply>(to: &mut UnixStream, request: u32, reply: &R) -> Result<(), protocol::ProtocolError> {
    protocol::write_reply_message(to, request, reply, VERSION)
}
