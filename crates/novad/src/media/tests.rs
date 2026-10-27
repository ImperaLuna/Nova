//! The media service against a real bus: a private `dbus-daemon` per test,
//! with fake MPRIS players that serve their properties and record the calls
//! made on them. Pure parts (which player is shown) are tested here too.

use super::*;
use crate::hub::{ClientQueue, Outgoing};
use crate::session_bus::TestBus;
use nova_protocol::LogLevel;
use nova_protocol::Topic;
use nova_test_support::{TempDir, wait_until};
use serde_json::Value;
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;
use zbus::{Variant, connection, interface};

/// How long a test waits for something the bus should do at once.
const WAIT: Duration = Duration::from_secs(4 * CALL_TIMEOUT.as_secs());

struct Fixture {
    dir: TempDir,
    bus: TestBus,
    ui: Receiver<Outgoing>,
    _ui_stream: UnixStream,
    service: Arc<NowPlaying>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("media-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let id = hub.register(queue);
        hub.subscribe(id, Topic::Media);
        let sources = Sources { bus: Bus::Address(bus.address.clone()), cache_dir: dir.join("covers") };
        let service = NowPlaying::start(sources, None, hub, logger);
        Self { dir, bus, ui, _ui_stream: ui_stream, service }
    }

    /// The next media event whose data `check` accepts.
    fn media_until(&self, what: &str, check: impl Fn(&Value) -> bool) -> Value {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            let media = crate::hub::next_named(
                &self.ui,
                "media",
                deadline.saturating_duration_since(std::time::Instant::now()),
            );
            if check(&media) {
                return media;
            }
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}: {media}");
        }
    }

    /// Starts a fake player with `fake`'s state on the test bus.
    fn player(&self, name: &str, fake: &Fake) -> Connection {
        let root = Root(fake.clone());
        let player = PlayerIface(fake.clone());
        let connection = zbus::block_on(
            connection::Builder::address(self.bus.address.as_str())
                .serve_at(MPRIS_PATH, root)
                .serve_at(MPRIS_PATH, player)
                .build(),
        )
        .unwrap();
        zbus::block_on(connection.request_name(format!("{MPRIS_PREFIX}{name}"))).unwrap();
        connection
    }
}

/// What a fake player says, and the calls made on it.
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);

#[derive(Default)]
struct FakeState {
    identity: String,
    desktop_entry: String,
    status: String,
    title: String,
    artist: String,
    art_url: String,
    length_us: i64,
    position_us: i64,
    can_seek: bool,
    calls: Vec<String>,
}

impl Fake {
    fn new(identity: &str, status: &str) -> Self {
        let fake = Fake::default();
        {
            let mut state = lock(&fake.0);
            state.identity = identity.into();
            state.desktop_entry = identity.to_lowercase();
            state.status = status.into();
            state.title = "Song".into();
            state.artist = "Artist".into();
            state.length_us = 200_000_000;
            state.can_seek = true;
        }
        fake
    }

    fn set(&self, change: impl FnOnce(&mut FakeState)) {
        change(&mut lock(&self.0));
    }

    fn calls(&self) -> Vec<String> {
        lock(&self.0).calls.clone()
    }

    fn record(&self, call: String) {
        lock(&self.0).calls.push(call);
    }
}

/// Tells the bus that `connection`'s player changed. Its reading is taken
/// again, so the signal's own list of changes is left empty.
fn announce(connection: &Connection) {
    zbus::block_on(connection.emit_signal(
        None::<&str>,
        MPRIS_PATH,
        PROPERTIES_INTERFACE,
        "PropertiesChanged",
        &(PLAYER_INTERFACE, HashMap::<String, OwnedVariant>::new(), Vec::<String>::new()),
    ))
    .unwrap();
}

struct Root(Fake);

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        lock(&self.0.0).identity.clone()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        lock(&self.0.0).desktop_entry.clone()
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    fn raise(&self) {
        self.0.record("Raise".into());
    }
}

struct PlayerIface(Fake);

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerIface {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        lock(&self.0.0).status.clone()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedVariant> {
        let state = lock(&self.0.0);
        let text = |value: &str| OwnedVariant::try_from(Variant::from(value)).unwrap();
        HashMap::from([
            ("xesam:title".to_string(), text(&state.title)),
            ("xesam:artist".to_string(), OwnedVariant::try_from(Variant::from(vec![state.artist.clone()])).unwrap()),
            ("mpris:artUrl".to_string(), text(&state.art_url)),
            ("mpris:length".to_string(), OwnedVariant::try_from(Variant::from(state.length_us)).unwrap()),
            (
                "mpris:trackid".to_string(),
                OwnedVariant::try_from(Variant::from(ObjectPath::try_from("/track/1").unwrap())).unwrap(),
            ),
        ])
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        lock(&self.0.0).position_us
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        lock(&self.0.0).can_seek
    }

    fn play_pause(&self) {
        self.0.record("PlayPause".into());
    }

    fn play(&self) {
        self.0.record("Play".into());
    }

    fn pause(&self) {
        self.0.record("Pause".into());
    }

    fn next(&self) {
        self.0.record("Next".into());
    }

    fn previous(&self) {
        self.0.record("Previous".into());
    }

    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        self.0.record(format!("SetPosition {track} {position}"));
    }
}

/// The player ids of an event, in order.
fn ids(media: &Value) -> Vec<String> {
    media["players"].as_array().unwrap().iter().map(|player| player["id"].as_str().unwrap().to_string()).collect()
}

fn id_of(name: &str) -> String {
    format!("{MPRIS_PREFIX}{name}")
}

#[test]
fn a_player_that_appears_is_shown_with_its_track() {
    let fixture = Fixture::new("appears");
    let fake = Fake::new("Fake", "Playing");
    let _player = fixture.player("one", &fake);
    let media = fixture.media_until("the player", |media| ids(media) == [id_of("one")]);
    assert_eq!(media["shown"], id_of("one"));
    assert_eq!(media["players"][0]["title"], "Song");
    assert_eq!(media["players"][0]["artist"], "Artist");
    assert_eq!(media["players"][0]["status"], "playing");
    assert_eq!(media["players"][0]["identity"], "Fake");
    assert_eq!(media["players"][0]["can_seek"], true);
}

#[test]
fn pausing_and_resuming_is_published() {
    let fixture = Fixture::new("pause");
    let fake = Fake::new("Fake", "Playing");
    let player = fixture.player("one", &fake);
    fixture.media_until("the player", |media| !ids(media).is_empty());
    fake.set(|state| state.status = "Paused".into());
    announce(&player);
    let paused = fixture.media_until("the pause", |media| media["players"][0]["status"] == "paused");
    assert_eq!(paused["shown"], id_of("one"));
}

#[test]
fn the_newest_playing_player_is_shown_until_the_user_picks_another() {
    let fixture = Fixture::new("choice");
    let first = Fake::new("First", "Playing");
    let second = Fake::new("Second", "Paused");
    let first_player = fixture.player("first", &first);
    let second_player = fixture.player("second", &second);
    // Playing, so the first one is shown even though the second came later.
    fixture.media_until("both players, the first shown", |media| {
        ids(media).len() == 2 && media["shown"] == id_of("first")
    });

    fixture.service.show(&id_of("second")).unwrap();
    fixture.media_until("the pick", |media| media["shown"] == id_of("second"));

    // Another player starting to play takes the pill back.
    second.set(|state| state.status = "Playing".into());
    announce(&second_player);
    fixture.media_until("the newest player", |media| media["shown"] == id_of("second"));
    first.set(|state| state.status = "Paused".into());
    announce(&first_player);
    fixture.media_until("the first pausing", |media| media["players"][0]["status"] == "paused");
}

#[test]
fn a_player_that_leaves_the_bus_is_forgotten() {
    let fixture = Fixture::new("leaves");
    let fake = Fake::new("Fake", "Playing");
    let player = fixture.player("one", &fake);
    fixture.media_until("the player", |media| !ids(media).is_empty());
    drop(player);
    let gone = fixture.media_until("the player leaving", |media| ids(media).is_empty());
    assert_eq!(gone["shown"], Value::Null);
}

#[test]
fn controls_reach_the_player_and_seeks_are_clamped() {
    let fixture = Fixture::new("controls");
    let fake = Fake::new("Fake", "Playing");
    let _player = fixture.player("one", &fake);
    fixture.media_until("the player", |media| !ids(media).is_empty());

    fixture.service.control(&id_of("one"), MediaAction::PlayPause).unwrap();
    fixture.service.control(&id_of("one"), MediaAction::Next).unwrap();
    fixture.service.seek(&id_of("one"), 900_000_000).unwrap();
    wait_until(WAIT, "the calls", || fake.calls().len() == 3);
    assert_eq!(fake.calls(), ["Pause", "Next", "SetPosition /track/1 200000000"]);
    assert!(fixture.service.control("org.mpris.MediaPlayer2.nobody", MediaAction::Next).is_err());
}

#[test]
fn a_player_that_cannot_seek_is_not_sent_one() {
    let fixture = Fixture::new("no-seek");
    let fake = Fake::new("Fake", "Playing");
    fake.set(|state| state.can_seek = false);
    let _player = fixture.player("one", &fake);
    fixture.media_until("the player", |media| !ids(media).is_empty());
    assert!(fixture.service.seek(&id_of("one"), 1).is_err());
}

#[test]
fn a_local_cover_is_passed_through_and_a_bad_one_is_refused() {
    let fixture = Fixture::new("cover");
    let picture = fixture.dir.join("cover.png");
    std::fs::write(&picture, b"not really a png").unwrap();
    let fake = Fake::new("Fake", "Playing");
    fake.set(|state| state.art_url = format!("file://{}", picture.display()));
    let _player = fixture.player("one", &fake);
    let media = fixture.media_until("the cover", |media| media["players"][0]["cover"].is_string());
    assert_eq!(media["players"][0]["cover"], picture.to_string_lossy().as_ref());

    let other = Fake::new("Other", "Playing");
    other.set(|state| state.art_url = "file:///no/such/cover.png".into());
    let _other_player = fixture.player("two", &other);
    let media = fixture.media_until("the second player", |media| ids(media).len() == 2);
    assert_eq!(media["players"][1]["cover"], Value::Null);
}

#[test]
fn long_text_is_capped() {
    let fixture = Fixture::new("long");
    let fake = Fake::new("Fake", "Playing");
    fake.set(|state| state.title = "t".repeat(1000));
    let _player = fixture.player("one", &fake);
    let media = fixture.media_until("the player", |media| !ids(media).is_empty());
    assert_eq!(media["players"][0]["title"].as_str().unwrap().chars().count(), MAX_TEXT_CHARS);
}

#[test]
fn focusing_a_player_with_no_window_and_no_raise_says_so() {
    let fixture = Fixture::new("no-raise");
    let fake = Fake::new("Fake", "Playing");
    let _player = fixture.player("one", &fake);
    fixture.media_until("the player", |media| !ids(media).is_empty());
    // No compositor here, and the fake can't raise, so nothing is focused or called.
    assert_eq!(fixture.service.focus(&id_of("one"), None), Ok(false));
    assert!(fake.calls().is_empty());
}

#[test]
fn the_shown_player_is_the_one_that_started_playing_last() {
    let mut state = State::default();
    let player = |id: &str, status, activated| Player {
        id: id.into(),
        unique: format!(":1.{activated}"),
        reading: Reading { status, ..reading_for(id) },
        read_at_ms: 0,
        cover: None,
        activated,
    };
    state.players = vec![
        player("a", MediaStatus::Playing, 1),
        player("b", MediaStatus::Playing, 3),
        player("c", MediaStatus::Paused, 9),
    ];
    assert_eq!(state.shown_id(), Some("b"));
    state.players = vec![player("a", MediaStatus::Paused, 4), player("c", MediaStatus::Paused, 9)];
    assert_eq!(state.shown_id(), Some("c"));
    state.players = vec![player("a", MediaStatus::Stopped, 4)];
    assert_eq!(state.shown_id(), None);
}

/// A reading with empty fields, for tests of the choice only.
fn reading_for(id: &str) -> Reading {
    Reading {
        identity: id.into(),
        desktop_entry: String::new(),
        can_raise: false,
        status: MediaStatus::Stopped,
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        art_url: String::new(),
        track_id: String::new(),
        length_us: 0,
        position_us: 0,
        rate: 1.0,
        can_next: false,
        can_previous: false,
        can_play: false,
        can_pause: false,
        can_seek: false,
    }
}
