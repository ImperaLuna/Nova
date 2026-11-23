//! The media players against a real bus: a private `dbus-daemon` per test,
//! with fake players that serve the two MPRIS interfaces the way players
//! do, and say when something changed with `PropertiesChanged`.

use super::*;
use nova_hub::{ClientQueue, Outgoing};
use nova_session_bus::TestBus;
use nova_test_support::{TempDir, wait_until};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;
use zbus::interface;
use zbus::wire::{ObjectPath, OwnedVariant, Variant};

/// The port, for players whose process says nothing: the fake players run
/// in the test's own process, which no copy of `/proc` has.
fn no_names(_: &Path, _: Option<u32>) -> Vec<String> {
    Vec::new()
}

/// The port, with what the tray's lookup finds for Zen started from a
/// terminal (`.zen-beta-wrapped`, in kitty's unit): novad's
/// `media_finds_the_process_names_the_tray_does` checks it does. The fake
/// players run in this process.
fn zen_from_a_terminal(_: &Path, pid: Option<u32>) -> Vec<String> {
    match pid == Some(std::process::id()) {
        true => vec!["zen-beta".into(), "kitty".into()],
        false => Vec::new(),
    }
}

/// How long a test waits for something the bus should do at once. Longer
/// than a read may take (two calls, each up to its timeout).
const WAIT: Duration = Duration::from_secs(4 * CALL_TIMEOUT.as_secs());
const SPOTIFY: &str = "org.mpris.MediaPlayer2.spotify";
const FIREFOX: &str = "org.mpris.MediaPlayer2.firefox.instance_1_42";

struct Fixture {
    dir: TempDir,
    bus: TestBus,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    _ui_stream: UnixStream,
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
        hub.subscribe(id, nova_media_api::TOPIC);
        Self { dir, bus, hub, logger, ui, _ui_stream: ui_stream }
    }

    fn media(&self) -> Arc<Media> {
        Media::start(self.sources(), AppIndex::of(None, &self.logger), Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    fn sources(&self) -> Sources {
        let bus = Bus::Address(self.bus.address.clone());
        Sources { bus, covers: self.dir.join("media"), proc: self.dir.join("proc"), process_names: no_names }
    }

    /// The next media event.
    fn next(&self) -> Value {
        nova_hub::next_named(&self.ui, "media", WAIT)
    }

    /// Media events until one `done` accepts, which it returns.
    fn until(&self, done: impl Fn(&Value) -> bool) -> Value {
        loop {
            let event = self.next();
            if done(&event) {
                return event;
            }
        }
    }

    /// Puts up a player as `name`, playing `track`.
    fn play(&self, name: &str, track: Track) -> FakePlayer {
        FakePlayer::start(self.bus.connect(), name, track)
    }
}

/// What a fake player plays and can do.
#[derive(Clone)]
struct Track {
    status: &'static str,
    title: String,
    artists: Vec<String>,
    art_url: Option<String>,
    length_us: i64,
    position_us: i64,
    can_seek: bool,
    can_raise: bool,
    desktop_entry: &'static str,
}

impl Track {
    fn playing(title: &str) -> Self {
        Self {
            status: "Playing",
            title: title.into(),
            artists: vec!["Nujabes".into(), "Shing02".into()],
            art_url: None,
            length_us: 100_000_000,
            position_us: 5_000_000,
            can_seek: true,
            can_raise: true,
            desktop_entry: "spotify",
        }
    }
}

/// What the fake player was asked to do, in order.
type Calls = Arc<Mutex<Vec<String>>>;

struct FakePlayer {
    connection: Connection,
    track: Arc<Mutex<Track>>,
    calls: Calls,
}

impl FakePlayer {
    fn start(connection: Connection, name: &str, track: Track) -> Self {
        let track = Arc::new(Mutex::new(track));
        let calls = Calls::default();
        zbus::block_on(async {
            let server = connection.object_server();
            server.at(PATH, Root { track: Arc::clone(&track), calls: Arc::clone(&calls) }).await.unwrap();
            server.at(PATH, Controls { track: Arc::clone(&track), calls: Arc::clone(&calls) }).await.unwrap();
            connection.request_name(name).await.unwrap();
        });
        Self { connection, track, calls }
    }

    /// Changes the track, and says so as players do.
    fn change(&self, change: impl FnOnce(&mut Track)) {
        change(&mut lock(&self.track));
        let changed: HashMap<String, OwnedVariant> = HashMap::new();
        let signal = (PLAYER_INTERFACE, changed, vec!["PlaybackStatus", "Metadata"]);
        zbus::block_on(self.connection.emit_signal(None::<&str>, PATH, PROPERTIES, "PropertiesChanged", &signal))
            .unwrap();
    }

    fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }
}

struct Root {
    track: Arc<Mutex<Track>>,
    calls: Calls,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Fake Player".into()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        lock(&self.track).desktop_entry.into()
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        lock(&self.track).can_raise
    }

    fn raise(&self) {
        lock(&self.calls).push("Raise".into());
    }
}

struct Controls {
    track: Arc<Mutex<Track>>,
    calls: Calls,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Controls {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        lock(&self.track).status.into()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedVariant> {
        let track = lock(&self.track).clone();
        let value = |value: Variant<'_>| OwnedVariant::try_from(value).unwrap();
        let mut metadata = HashMap::from([
            ("mpris:trackid".to_string(), value(ObjectPath::try_from("/track/1").unwrap().into())),
            ("xesam:title".to_string(), value(track.title.as_str().into())),
            ("xesam:artist".to_string(), value(track.artists.clone().into())),
            ("mpris:length".to_string(), value(track.length_us.into())),
        ]);
        if let Some(url) = &track.art_url {
            metadata.insert("mpris:artUrl".into(), value(url.as_str().into()));
        }
        metadata
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        lock(&self.track).position_us
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
        lock(&self.track).can_seek
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    fn play_pause(&self) {
        lock(&self.calls).push("PlayPause".into());
    }

    fn next(&self) {
        lock(&self.calls).push("Next".into());
    }

    fn previous(&self) {
        lock(&self.calls).push("Previous".into());
    }

    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        lock(&self.calls).push(format!("SetPosition {track} {position}"));
    }
}

/// The player `id` in a media event.
fn player<'a>(event: &'a Value, id: &str) -> &'a Value {
    event["players"].as_array().unwrap().iter().find(|player| player["id"] == id).unwrap_or(&Value::Null)
}

#[test]
fn a_player_shows_changes_and_goes() {
    let fixture = Fixture::new("one");
    let _media = fixture.media();
    let spotify = fixture.play(SPOTIFY, Track::playing("Battlecry"));
    let event = fixture.until(|event| event["shown"] == SPOTIFY);
    let shown = player(&event, SPOTIFY);
    assert_eq!(shown["title"], "Battlecry");
    assert_eq!(shown["artist"], "Nujabes, Shing02");
    assert_eq!(shown["identity"], "Fake Player");
    assert_eq!(shown["status"], "playing");
    assert_eq!(shown["length_us"], 100_000_000);
    assert_eq!(shown["position_us"], 5_000_000);
    assert!(shown["position_at_ms"].as_i64().unwrap() > 0);
    assert!(shown.get("app").is_none(), "without the launcher, no app");

    spotify.change(|track| track.status = "Paused");
    let event = fixture.until(|event| player(event, SPOTIFY)["status"] == "paused");
    assert_eq!(event["shown"], SPOTIFY, "a paused player still shows");

    drop(spotify);
    assert_eq!(fixture.until(|event| event["players"] == json!([])), json!({"players": []}));
}

#[test]
fn the_latest_to_play_shows_unless_the_user_picks_another() {
    let fixture = Fixture::new("two");
    let media = fixture.media();
    let spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    fixture.until(|event| event["shown"] == SPOTIFY);
    let firefox =
        fixture.play(FIREFOX, Track { status: "Paused", desktop_entry: "firefox", ..Track::playing("A video") });
    let event = fixture.until(|event| player(event, FIREFOX)["status"] == "paused");
    assert_eq!(event["shown"], SPOTIFY, "the one playing wins over a paused one");

    firefox.change(|track| track.status = "Playing");
    fixture.until(|event| event["shown"] == FIREFOX);

    media.show(SPOTIFY).unwrap();
    fixture.until(|event| event["shown"] == SPOTIFY);
    // Changes to the shown players don't undo the pick...
    spotify.change(|track| track.title = "Battlecry".into());
    let event = fixture.until(|event| player(event, SPOTIFY)["title"] == "Battlecry");
    assert_eq!(event["shown"], SPOTIFY);
    // ...but another player starting to play does.
    firefox.change(|track| track.status = "Paused");
    fixture.until(|event| player(event, FIREFOX)["status"] == "paused");
    firefox.change(|track| track.status = "Playing");
    fixture.until(|event| event["shown"] == FIREFOX);

    // With the playing one gone, the paused one shows; with none, nothing.
    spotify.change(|track| track.status = "Paused");
    fixture.until(|event| player(event, SPOTIFY)["status"] == "paused");
    drop(firefox);
    fixture.until(|event| event["shown"] == SPOTIFY && player(event, FIREFOX).is_null());
    spotify.change(|track| track.status = "Stopped");
    let event = fixture.until(|event| player(event, SPOTIFY)["status"] == "stopped");
    assert!(event.get("shown").is_none(), "{event}");
    assert!(media.show("org.mpris.MediaPlayer2.nobody").unwrap_err().contains("no player"));
}

#[test]
fn a_pick_ends_with_its_player() {
    let fixture = Fixture::new("pick-ends");
    let media = fixture.media();
    let _spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    fixture.until(|event| event["shown"] == SPOTIFY);
    let paused = || Track { status: "Paused", desktop_entry: "firefox", ..Track::playing("A video") };
    let firefox = fixture.play(FIREFOX, paused());
    fixture.until(|event| player(event, FIREFOX)["status"] == "paused");
    media.show(FIREFOX).unwrap();
    fixture.until(|event| event["shown"] == FIREFOX);
    drop(firefox);
    fixture.until(|event| event["shown"] == SPOTIFY);
    // Back under the same name, it's a new player: the playing one stays.
    let _firefox = fixture.play(FIREFOX, paused());
    let event = fixture.until(|event| player(event, FIREFOX)["status"] == "paused");
    assert_eq!(event["shown"], SPOTIFY);
}

#[test]
fn a_paused_pick_that_moves_on_ends() {
    // A browser keeps one player for all its tabs: closing the picked tab
    // leaves it paused on another tab's media, or on none.
    let fixture = Fixture::new("pick-moves-on");
    let media = fixture.media();
    let _spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    fixture.until(|event| event["shown"] == SPOTIFY);
    let firefox =
        fixture.play(FIREFOX, Track { status: "Paused", desktop_entry: "firefox", ..Track::playing("A video") });
    fixture.until(|event| player(event, FIREFOX)["status"] == "paused");
    media.show(FIREFOX).unwrap();
    fixture.until(|event| event["shown"] == FIREFOX);
    // Its position moving isn't moving on.
    firefox.change(|track| track.position_us = 9_000_000);
    let event = fixture.until(|event| player(event, FIREFOX)["position_us"] == 9_000_000);
    assert_eq!(event["shown"], FIREFOX);
    firefox.change(|track| track.title = String::new());
    fixture.until(|event| event["shown"] == SPOTIFY);
}

#[test]
fn a_short_clip_is_left_out_until_its_track_is_longer() {
    // A chat app's notification sound: a 6 second clip, as Ferdium plays.
    const CHAT: &str = "org.mpris.MediaPlayer2.chromium.instance_7";
    let fixture = Fixture::new("short-clip");
    let media = fixture.media();
    let _spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    fixture.until(|event| event["shown"] == SPOTIFY);
    media.show(SPOTIFY).unwrap();
    let clip = Track { length_us: 6_000_000, position_us: 0, artists: Vec::new(), ..Track::playing("WhatsApp") };
    let chat = fixture.play(CHAT, Track { desktop_entry: "ferdium", ..clip });
    // Read, and refused: no command reaches it.
    wait_until(WAIT, "the clip is read", || {
        media.control(CHAT, MediaAction::PlayPause).is_err_and(|e| e.contains("short clip"))
    });
    assert!(media.show(CHAT).is_err());
    assert!(chat.calls().is_empty());
    // Not listed, and its start ends no pick: nothing new to publish.
    let now = media.shared.event(&lock(&media.shared.state));
    assert_eq!(now.players.iter().map(|player| player.id.as_str()).collect::<Vec<_>>(), [SPOTIFY]);
    assert_eq!(now.shown.as_deref(), Some(SPOTIFY));
    assert_eq!(lock(&media.shared.state).picked.as_deref(), Some(SPOTIFY));

    // A longer track (a voice note): listed again, and it can be shown.
    chat.change(|track| track.length_us = 40_000_000);
    let event = fixture.until(|event| !player(event, CHAT).is_null());
    assert_eq!(event["shown"], SPOTIFY, "the pick holds");
    media.show(CHAT).unwrap();
    fixture.until(|event| event["shown"] == CHAT);
    // Back to a clip: it can't keep the pill, even picked.
    chat.change(|track| track.length_us = 3_000_000);
    fixture.until(|event| player(event, CHAT).is_null() && event["shown"] == SPOTIFY);
}

#[test]
fn a_player_is_found_by_its_process_when_its_desktop_entry_is_wrong() {
    // Zen's player says it's `zen`; its app is zen-beta.desktop, and its
    // process (`.zen-beta-wrapped`) and windows are `zen-beta`.
    let fixture = Fixture::new("process");
    let share = fixture.dir.join("share");
    std::fs::create_dir_all(share.join("applications")).unwrap();
    std::fs::write(
        share.join("applications/zen-beta.desktop"),
        "[Desktop Entry]\nType=Application\nName=Zen Browser (Beta)\nExec=true\nStartupWMClass=zen-beta\n\
         Categories=Network;WebBrowser;\n",
    )
    .unwrap();
    let apps = AppIndex::of(Some(&share), &fixture.logger);
    wait_until(WAIT, "the index to find zen-beta.desktop", || apps.find_app(&["zen-beta".into()]).is_some());
    let sources = Sources { process_names: zen_from_a_terminal, ..fixture.sources() };
    let media = Media::start(sources, apps, Arc::clone(&fixture.hub), Arc::clone(&fixture.logger));

    let _zen = fixture.play(FIREFOX, Track { desktop_entry: "zen", ..Track::playing("A video") });
    let event = fixture.until(|event| event["shown"] == FIREFOX);
    let zen = player(&event, FIREFOX);
    assert_eq!(zen["app"]["id"], "zen-beta.desktop");
    assert_eq!(zen["browser"], true, "so the pill leaves out the channel");
    let ids = media.window_ids(FIREFOX).unwrap();
    assert!(ids.contains(&"zen".into()) && ids.contains(&"zen-beta".into()), "{ids:?}");
    assert!(!ids.contains(&"kitty".into()), "the terminal's windows aren't Zen's: {ids:?}");
}

#[test]
fn playerctld_is_not_a_player_of_its_own() {
    // It stands in for the player that played last.
    assert!(super::is_player(SPOTIFY) && super::is_player(FIREFOX));
    assert!(!super::is_player("org.mpris.MediaPlayer2.playerctld"));
    assert!(!super::is_player("org.freedesktop.Notifications"));
}

#[test]
fn controls_reach_the_player_and_seeking_stays_in_the_track() {
    let fixture = Fixture::new("controls");
    let media = fixture.media();
    let spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    fixture.until(|event| event["shown"] == SPOTIFY);

    for action in [MediaAction::PlayPause, MediaAction::Next, MediaAction::Previous] {
        media.control(SPOTIFY, action).unwrap();
    }
    media.seek(SPOTIFY, 30_000_000).unwrap();
    media.seek(SPOTIFY, 900_000_000).unwrap();
    media.seek(SPOTIFY, -5).unwrap();
    assert_eq!(
        spotify.calls(),
        [
            "PlayPause",
            "Next",
            "Previous",
            "SetPosition /track/1 30000000",
            "SetPosition /track/1 100000000",
            "SetPosition /track/1 0",
        ]
    );

    assert_eq!(media.raise(SPOTIFY), Ok(true));
    assert_eq!(spotify.calls().last().unwrap(), "Raise");
    assert_eq!(media.window_ids(SPOTIFY), Ok(vec!["spotify".to_string()]), "its desktop entry, without the launcher");

    spotify.change(|track| {
        track.can_seek = false;
        track.can_raise = false;
    });
    fixture.until(|event| player(event, SPOTIFY)["can_seek"] == false);
    assert!(media.seek(SPOTIFY, 1).unwrap_err().contains("can't seek"));
    assert_eq!(media.raise(SPOTIFY), Ok(false));
}

#[test]
fn seeked_moves_the_position() {
    let fixture = Fixture::new("seeked");
    let _media = fixture.media();
    let spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    fixture.until(|event| event["shown"] == SPOTIFY);
    zbus::block_on(spotify.connection.emit_signal(None::<&str>, PATH, PLAYER_INTERFACE, "Seeked", &(42_000_000i64,)))
        .unwrap();
    fixture.until(|event| player(event, SPOTIFY)["position_us"] == 42_000_000);
}

#[test]
fn covers_are_checked_files() {
    let fixture = Fixture::new("covers");
    let _media = fixture.media();
    let cover = fixture.dir.join("cover.png");
    std::fs::write(&cover, b"png").unwrap();
    let track = Track { art_url: Some(format!("file://{}", cover.display())), ..Track::playing("Feather") };
    let spotify = fixture.play(SPOTIFY, track);
    let event = fixture.until(|event| event["shown"] == SPOTIFY);
    assert_eq!(player(&event, SPOTIFY)["cover"], json!(cover.display().to_string()));

    // A path that climbs out of where it says, and a folder, are refused.
    let sneaky = format!("file://{}/../{}", fixture.dir.join("media").display(), "cover.png");
    spotify.change(|track| {
        track.art_url = Some(sneaky);
        track.title = "Climbing".into();
    });
    let event = fixture.until(|event| player(event, SPOTIFY)["title"] == "Climbing");
    assert!(player(&event, SPOTIFY).get("cover").is_none(), "{event}");
    spotify.change(|track| {
        track.art_url = Some(format!("file://{}", fixture.dir.display()));
        track.title = "A folder".into();
    });
    let event = fixture.until(|event| player(event, SPOTIFY)["title"] == "A folder");
    assert!(player(&event, SPOTIFY).get("cover").is_none(), "{event}");
}

#[test]
fn long_text_is_capped() {
    let fixture = Fixture::new("long-text");
    let _media = fixture.media();
    let long = "x".repeat(player::MAX_TEXT_CHARS * 10);
    let track = Track { artists: vec![long.clone(); 4], ..Track::playing(&long) };
    let _spotify = fixture.play(SPOTIFY, track);
    let event = fixture.until(|event| event["shown"] == SPOTIFY);
    let shown = player(&event, SPOTIFY);
    assert_eq!(shown["title"].as_str().unwrap().chars().count(), player::MAX_TEXT_CHARS);
    assert_eq!(shown["artist"].as_str().unwrap().chars().count(), player::MAX_TEXT_CHARS);
}

#[test]
fn players_already_there_are_found_at_start() {
    let fixture = Fixture::new("already");
    let _spotify = fixture.play(SPOTIFY, Track::playing("Feather"));
    let media = fixture.media();
    wait_until(WAIT, "the player to be read", || media.window_ids(SPOTIFY).is_ok());
    fixture.until(|event| event["shown"] == SPOTIFY);
}
