//! The service against a stand-in sound server ([`FakePulse`]) that
//! answers from recordings of real ones.

use super::*;
use crate::fake_pulse::{FakePulse, recorded, recording};

/// Writes what the sound server running now lists as a recording, to the
/// file `NOVA_SOUND_RECORD` names:
/// `NOVA_SOUND_RECORD=$PWD/name.pulse cargo test -p nova-sound record -- --ignored`
#[test]
#[ignore = "records the machine's own sound server"]
fn record_the_running_server() {
    let file = std::env::var("NOVA_SOUND_RECORD").expect("NOVA_SOUND_RECORD names the file to write");
    let listing = Link::connect(&Sources::system()).and_then(|mut link| link.listing()).unwrap();
    assert_eq!(recorded(&recording(&listing)), listing, "the recording reads back as what was listed");
    std::fs::write(file, recording(&listing)).unwrap();
    println!("{:#?}", reading::sound(&listing));
}

use nova_carrier::log::LogLevel;
use nova_hub::{ClientQueue, Outgoing};
use nova_test_support::{TempDir, wait_until};
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

/// A PipeWire with two null devices (`Cloud-Speakers` at 40% and a muted
/// virtual mic, both in use), `Tunes` playing two streams (one muted, one
/// at half volume), `Chimes` playing one, and `Talk` recording from the
/// mic. Its sources also hold the speakers' monitor.
const NULL_DEVICES: &str = include_str!("recorded/pipewire-null-devices.pulse");

/// How long a test waits for what should happen at once.
const WAIT: Duration = Duration::from_secs(5);
/// Long enough for something that shouldn't happen to have happened.
const QUIET: Duration = Duration::from_millis(300);
const EVENT: &str = "sound";

struct Fixture {
    dir: TempDir,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    client: ClientId,
    _ui_stream: UnixStream,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("sound-{name}"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let client = hub.register(queue);
        hub.subscribe(client, nova_sound_api::TOPIC);
        Self { dir, hub, logger, ui, client, _ui_stream: ui_stream }
    }

    fn socket(&self) -> std::path::PathBuf {
        self.dir.join("native")
    }

    /// A server answering from the recording.
    fn server(&self) -> FakePulse {
        FakePulse::start(&self.socket(), recorded(NULL_DEVICES))
    }

    fn sound(&self) -> Arc<Sound> {
        let sources = Sources { socket: Some(self.socket()), cookies: Vec::new() };
        Sound::start(sources, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    /// The next sound event.
    fn next(&self) -> Value {
        nova_hub::next_named(&self.ui, EVENT, WAIT)
    }

    fn nothing_comes(&self) {
        let event = nova_hub::named_within(&self.ui, EVENT, QUIET);
        assert_eq!(event, None, "a sound event came");
    }
}

#[test]
fn it_publishes_what_the_server_lists() {
    let fixture = Fixture::new("lists");
    let _server = fixture.server();
    let _sound = fixture.sound();
    let sound = fixture.next();
    assert_eq!(sound["available"], true);
    assert_eq!(
        sound["outputs"],
        json!([{"id": 53, "name": "Cloud-Speakers", "kind": "speakers", "volume": sound["outputs"][0]["volume"], "muted": false, "current": true}])
    );
    let volume = sound["outputs"][0]["volume"].as_f64().unwrap();
    assert!((volume - 0.4).abs() < 0.001, "{volume}");
    // The speakers' monitor isn't an input.
    let inputs = sound["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1, "{inputs:?}");
    assert_eq!(
        (&inputs[0]["id"], &inputs[0]["muted"], &inputs[0]["current"]),
        (&json!(60), &json!(true), &json!(true))
    );
    // Tunes' two streams are one app: as loud as the louder, and not muted
    // while one of them plays.
    assert_eq!(
        sound["apps"],
        json!([
            {"id": 66, "name": "Tunes", "volume": 1.0, "muted": false},
            {"id": 89, "name": "Chimes", "volume": 1.0, "muted": false},
        ])
    );
    assert_eq!(sound["recorders"], json!(["Talk"]));
}

#[test]
fn a_change_is_published_whoever_made_it() {
    let fixture = Fixture::new("change");
    let server = fixture.server();
    let _sound = fixture.sound();
    assert_eq!(fixture.next()["outputs"][0]["muted"], false);
    server.change(|listing| listing.sinks[0].muted = true);
    assert_eq!(fixture.next()["outputs"][0]["muted"], true);
    // An app that stops playing leaves the list.
    server.change(|listing| listing.sink_inputs.retain(|stream| stream.index != 89));
    assert_eq!(fixture.next()["apps"].as_array().unwrap().len(), 1);
    // The output going leaves none in use.
    server.change(|listing| listing.sinks.clear());
    assert_eq!(fixture.next()["outputs"], json!([]));
}

#[test]
fn what_clients_cant_see_change_isnt_published_again() {
    let fixture = Fixture::new("same");
    let server = fixture.server();
    let _sound = fixture.sound();
    fixture.next();
    let before = server.listings();
    server.say_changed(1);
    wait_until(WAIT, "the listing is read again", || server.listings() > before);
    fixture.nothing_comes();
}

#[test]
fn a_new_subscriber_gets_the_sound_as_it_is() {
    let fixture = Fixture::new("subscriber");
    let _server = fixture.server();
    let sound = fixture.sound();
    let published = fixture.next();
    sound.send_state(fixture.client);
    assert_eq!(fixture.next(), published);
}

#[test]
fn without_a_server_it_says_so_and_finds_one_that_starts() {
    let fixture = Fixture::new("late");
    let sound = fixture.sound();
    sound.send_state(fixture.client);
    assert_eq!(fixture.next(), json!({"available": false, "outputs": [], "inputs": [], "apps": [], "recorders": []}));
    let _server = fixture.server();
    assert_eq!(fixture.next()["available"], true);
}

#[test]
fn a_server_that_restarts_reads_as_no_sound_until_it_is_back() {
    let fixture = Fixture::new("restart");
    let server = fixture.server();
    let _sound = fixture.sound();
    assert_eq!(fixture.next()["available"], true);
    server.hang_up();
    let gone = fixture.next();
    assert_eq!((&gone["available"], &gone["outputs"]), (&json!(false), &json!([])));
    assert_eq!(fixture.next()["available"], true);
    // And follows it again.
    server.change(|listing| listing.sinks[0].muted = true);
    assert_eq!(fixture.next()["outputs"][0]["muted"], true);
}

#[test]
fn once_stopped_it_publishes_nothing_and_hangs_up() {
    let fixture = Fixture::new("stop");
    let server = fixture.server();
    let sound = fixture.sound();
    fixture.next();
    sound.stop();
    wait_until(WAIT, "it hangs up", || {
        server.say_changed(1);
        server.followers() == 0
    });
    server.change(|listing| listing.sinks[0].muted = true);
    fixture.nothing_comes();
}
