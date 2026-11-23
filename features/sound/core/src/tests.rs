//! The service against a stand-in sound server ([`FakePulse`]) that
//! answers from recordings of real ones.

use super::*;
use crate::fake_pulse::{FakePulse, recorded, recording};
use crate::server::Listing;
use nova_sound_api::{Kind, VOLUME_MAX};
use nova_volume_osd_api::VolumeKey;
use pulseaudio::protocol::{ChannelVolume, SinkInputInfo, Volume};

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
        let sources = Sources { socket: Some(self.socket()) };
        Sound::start(sources, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    /// The next sound event.
    fn next(&self) -> Value {
        nova_hub::next_named(&self.ui, EVENT, WAIT)
    }

    /// Sound events until one `done` accepts, which it returns: a command
    /// that tells the server two things may come back as one event or two.
    fn until(&self, done: impl Fn(&Value) -> bool) -> Value {
        loop {
            let sound = self.next();
            if done(&sound) {
                return sound;
            }
        }
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
fn an_app_playing_untouched_reads_as_full_volume() {
    let fixture = Fixture::new("untouched");
    let server = fixture.server();
    let _sound = fixture.sound();
    fixture.next();
    // Chimes turned all the way down reads as that.
    fn chimes(listing: &mut Listing) -> &mut SinkInputInfo {
        listing.sink_inputs.iter_mut().find(|stream| stream.index == 89).unwrap()
    }
    server.change(|listing| chimes(listing).cvolume = ChannelVolume::muted(2));
    assert_eq!(fixture.next()["apps"][1]["volume"], 0.0);
    // Without a volume of its own, like passed-through surround sound, it
    // plays as it is whatever the number says.
    server.change(|listing| chimes(listing).has_volume = false);
    assert_eq!(fixture.next()["apps"][1]["volume"], 1.0);
}

#[test]
fn a_server_that_lists_nothing_is_tried_again_ever_later() {
    let fixture = Fixture::new("broken");
    let server = fixture.server();
    server.refuse_listings();
    let _sound = fixture.sound();
    // The second try comes a second after the first, the third two
    // seconds later: three inside the time two fixed waits would take
    // means the waits didn't grow.
    wait_until(WAIT, "it tries again", || server.listings() >= 2);
    nova_test_support::settle(FIRST_RETRY + FIRST_RETRY / 5);
    assert_eq!(server.listings(), 2);
    fixture.nothing_comes();
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
    // Nor does it change anything any more.
    let mute = Command::SetSoundMuted { of: Kind::Output, id: 53, muted: true };
    assert!(sound.change(&mute).is_err());
}

fn close(value: &Value, expected: f64) -> bool {
    value.as_f64().is_some_and(|value| (value - expected).abs() < 0.001)
}

#[test]
fn a_volume_set_by_hand_unmutes_and_comes_back_as_an_event() {
    let fixture = Fixture::new("volume");
    let _server = fixture.server();
    let sound = fixture.sound();
    assert_eq!(fixture.next()["inputs"][0]["muted"], true);
    sound.change(&Command::SetSoundVolume { of: Kind::Input, id: 60, volume: 0.25 }).unwrap();
    fixture.until(|sound| close(&sound["inputs"][0]["volume"], 0.25) && sound["inputs"][0]["muted"] == false);
    sound.change(&Command::SetSoundVolume { of: Kind::Output, id: 53, volume: 1.0 }).unwrap();
    fixture.until(|sound| close(&sound["outputs"][0]["volume"], 1.0));
    sound.change(&Command::SetSoundMuted { of: Kind::Output, id: 53, muted: true }).unwrap();
    assert_eq!(fixture.next()["outputs"][0]["muted"], true);
}

#[test]
fn the_capsules_volume_leaves_mute_as_it_is() {
    let fixture = Fixture::new("level");
    let _server = fixture.server();
    let sound = fixture.sound();
    assert_eq!(fixture.next()["inputs"][0]["muted"], true);
    sound.change(&Command::SetSoundLevel { of: Kind::Input, id: 60, volume: 0.25 }).unwrap();
    let set = fixture.until(|sound| close(&sound["inputs"][0]["volume"], 0.25));
    assert_eq!(set["inputs"][0]["muted"], true);
    let refused = sound.change(&Command::SetSoundLevel { of: Kind::Output, id: 53, volume: 1.5 });
    assert_eq!(refused, Err(format!("a volume is 0 to {VOLUME_MAX}, not 1.5")));
}

#[test]
fn an_apps_volume_and_mute_reach_all_its_streams() {
    let fixture = Fixture::new("app");
    let server = fixture.server();
    let sound = fixture.sound();
    fixture.next();
    let tunes = |server: &FakePulse| -> Vec<(f32, bool)> {
        let streams = server.listing().sink_inputs;
        streams.iter().filter(|stream| stream.index != 89).map(|s| (reading::level(&s.cvolume), s.muted)).collect()
    };
    assert_eq!(tunes(&server).len(), 2);
    assert!(tunes(&server).iter().any(|(_, muted)| *muted), "one of the recorded streams is muted");
    sound.change(&Command::SetSoundVolume { of: Kind::App, id: 66, volume: 0.5 }).unwrap();
    fixture.until(|sound| close(&sound["apps"][0]["volume"], 0.5));
    for (volume, muted) in tunes(&server) {
        assert!((volume - 0.5).abs() < 0.001 && !muted, "{volume} {muted}");
    }
    sound.change(&Command::SetSoundMuted { of: Kind::App, id: 66, muted: true }).unwrap();
    fixture.until(|sound| sound["apps"][0]["muted"] == true);
    assert!(tunes(&server).iter().all(|(_, muted)| *muted));
    // The other app was left alone.
    let chimes = server.listing().sink_inputs.into_iter().find(|stream| stream.index == 89).unwrap();
    assert!((reading::level(&chimes.cvolume) - 1.0).abs() < 0.001 && !chimes.muted);
}

#[test]
fn another_output_becomes_the_one_in_use() {
    let fixture = Fixture::new("use");
    let server = fixture.server();
    let sound = fixture.sound();
    fixture.next();
    // A second output appears, as when a headset is plugged in.
    server.change(|listing| {
        let mut headset = listing.sinks[0].clone();
        headset.index = 200;
        headset.name = c"headset".into();
        listing.sinks.push(headset);
    });
    let current = |sound: &Value| -> Vec<bool> {
        sound["outputs"].as_array().unwrap().iter().map(|output| output["current"] == true).collect()
    };
    assert_eq!(current(&fixture.next()), [true, false]);
    sound.change(&Command::UseSoundOutput { id: 200 }).unwrap();
    assert_eq!(current(&fixture.next()), [false, true]);
}

#[test]
fn what_cant_be_done_is_refused_with_the_reason() {
    let fixture = Fixture::new("refused");
    let server = fixture.server();
    let sound = fixture.sound();
    assert_eq!(sound.change(&Command::UseSoundOutput { id: 53 }), Err(NO_SERVER.into()), "nothing is listed yet");
    fixture.next();
    let refused = |command: Command| sound.change(&command).unwrap_err();
    assert_eq!(refused(Command::SetSoundMuted { of: Kind::Output, id: 7, muted: true }), "there is no output 7");
    assert_eq!(refused(Command::SetSoundMuted { of: Kind::App, id: 7, muted: true }), "no app plays as 7");
    // The speakers' monitor is a source, but not an input.
    let monitor = recorded(NULL_DEVICES).sources.into_iter().find(|source| source.monitor_of_sink_index.is_some());
    let monitor = monitor.unwrap().index;
    assert_eq!(refused(Command::UseSoundInput { id: monitor }), format!("there is no input {monitor}"));
    for volume in [-0.1, 1.5, f32::NAN] {
        let reason = refused(Command::SetSoundVolume { of: Kind::Output, id: 53, volume });
        assert!(reason.starts_with("a volume is 0 to 1"), "{reason}");
    }
    // An output that went since the last listing: the server says no, and
    // the next command still goes through.
    server.change_quietly(|listing| listing.sinks.clear());
    let reason = refused(Command::SetSoundMuted { of: Kind::Output, id: 53, muted: true });
    assert!(reason.starts_with("the sound server said"), "{reason}");
    sound.change(&Command::SetSoundMuted { of: Kind::Input, id: 60, muted: false }).unwrap();
    fixture.until(|sound| sound["inputs"][0]["muted"] == false);
}

#[test]
fn a_volume_keeps_the_balance_between_channels() {
    let mut channels = ChannelVolume::empty();
    channels.push(Volume::from_u32_clamped(Volume::NORM.as_u32() / 2));
    channels.push(Volume::NORM);
    let moved = reading::at_level(&channels, 0.375);
    let norm = Volume::NORM.as_u32();
    assert_eq!(moved.channels(), [Volume::from_u32_clamped(norm / 4), Volume::from_u32_clamped(norm / 2)]);
    // From silence there is no balance to keep.
    let moved = reading::at_level(&ChannelVolume::muted(2), 0.5);
    assert_eq!(moved.channels(), [Volume::from_u32_clamped(norm / 2); 2]);
}

#[test]
fn a_volume_key_steps_the_output_in_use_and_mutes_it() {
    let fixture = Fixture::new("keys");
    let _server = fixture.server();
    let sound = fixture.sound();
    assert!(close(&fixture.next()["outputs"][0]["volume"], 0.4));
    sound.press(VolumeKey::Up).unwrap();
    fixture.until(|sound| close(&sound["outputs"][0]["volume"], 0.45));
    sound.press(VolumeKey::Mute).unwrap();
    fixture.until(|sound| sound["outputs"][0]["muted"] == true);
    // A step leaves mute as it is.
    sound.press(VolumeKey::Down).unwrap();
    let stepped = fixture.until(|sound| close(&sound["outputs"][0]["volume"], 0.4));
    assert_eq!(stepped["outputs"][0]["muted"], true);
    sound.press(VolumeKey::Mute).unwrap();
    fixture.until(|sound| sound["outputs"][0]["muted"] == false);
}

#[test]
fn keys_pressed_faster_than_the_server_lists_each_take_a_step() {
    let fixture = Fixture::new("keys-fast");
    let _server = fixture.server();
    let sound = fixture.sound();
    assert!(close(&fixture.next()["outputs"][0]["volume"], 0.4));
    for _ in 0..3 {
        sound.press(VolumeKey::Up).unwrap();
    }
    fixture.until(|sound| close(&sound["outputs"][0]["volume"], 0.55));
    // Mute and unmute at once leave it as it was.
    sound.press(VolumeKey::Mute).unwrap();
    sound.press(VolumeKey::Mute).unwrap();
    sound.press(VolumeKey::Up).unwrap();
    let stepped = fixture.until(|sound| close(&sound["outputs"][0]["volume"], 0.6));
    assert_eq!(stepped["outputs"][0]["muted"], false);
}

#[test]
fn a_volume_key_at_full_changes_nothing() {
    let fixture = Fixture::new("keys-full");
    let _server = fixture.server();
    let sound = fixture.sound();
    fixture.next();
    sound.change(&Command::SetSoundVolume { of: Kind::Output, id: 53, volume: 1.0 }).unwrap();
    fixture.until(|sound| close(&sound["outputs"][0]["volume"], 1.0));
    sound.press(VolumeKey::Up).unwrap();
    fixture.nothing_comes();
}

#[test]
fn a_volume_key_without_an_output_in_use_says_so() {
    let fixture = Fixture::new("keys-none");
    let server = fixture.server();
    let sound = fixture.sound();
    assert_eq!(sound.press(VolumeKey::Up), Err(NO_SERVER.into()), "nothing is listed yet");
    fixture.next();
    server.change(|listing| listing.sinks.clear());
    fixture.next();
    assert_eq!(sound.press(VolumeKey::Up), Err("there is no output in use".into()));
}

/// The listing with a Bluetooth headset added, as BlueZ names its nodes.
fn add_headset(listing: &mut Listing) {
    let mut output = listing.sinks[0].clone();
    output.index = 300;
    output.name = c"bluez_output.AA_BB_CC_DD_EE_FF.1".into();
    listing.sinks.push(output);
    let mut input = listing.sources.iter().find(|source| source.monitor_of_sink_index.is_none()).unwrap().clone();
    input.index = 301;
    input.name = c"bluez_input.AA_BB_CC_DD_EE_FF.0".into();
    listing.sources.push(input);
}

fn in_use(listing: &Listing) -> (String, String) {
    let name = |name: &Option<std::ffi::CString>| name.as_deref().unwrap().to_string_lossy().into_owned();
    (name(&listing.server.default_sink_name), name(&listing.server.default_source_name))
}

#[test]
fn a_bluetooth_headset_that_appears_becomes_the_one_in_use() {
    let fixture = Fixture::new("bluetooth");
    let server = fixture.server();
    let _sound = fixture.sound();
    fixture.next();
    let before = in_use(&server.listing());
    server.change(add_headset);
    wait_until(WAIT, "the headset is in use", || {
        in_use(&server.listing())
            == ("bluez_output.AA_BB_CC_DD_EE_FF.1".to_owned(), "bluez_input.AA_BB_CC_DD_EE_FF.0".to_owned())
    });
    // Choosing the speakers again sticks: the headset is no longer new.
    let speakers = std::ffi::CString::new(before.0.clone()).unwrap();
    server.change(|listing| listing.server.default_sink_name = Some(speakers));
    nova_test_support::settle(QUIET);
    assert_eq!(in_use(&server.listing()).0, before.0);
}

#[test]
fn a_bluetooth_headset_there_from_the_start_is_left_as_the_server_chose() {
    let fixture = Fixture::new("bluetooth-start");
    let mut listing = recorded(NULL_DEVICES);
    add_headset(&mut listing);
    let server = FakePulse::start(&fixture.socket(), listing);
    let _sound = fixture.sound();
    let before = in_use(&server.listing());
    fixture.next();
    server.say_changed(1);
    nova_test_support::settle(QUIET);
    assert_eq!(in_use(&server.listing()), before);
}
