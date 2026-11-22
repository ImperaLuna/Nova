//! The notification server against a real bus: a private `dbus-daemon` per
//! test, with apps that send notifications the way libnotify does.

use super::*;
use futures_core::Stream;
use nova_hub::{ClientQueue, Outgoing};
use nova_session_bus::TestBus;
use nova_test_support::{TempDir, wait_until};
use serde_json::{Value, json};
use std::future::poll_fn;
use std::os::unix::net::UnixStream;
use std::pin::Pin;
use std::sync::mpsc::Receiver;
use zbus::{MatchRule, MessageStream};

/// How long a test waits for something the bus should do at once.
const WAIT: Duration = Duration::from_secs(5);

struct Fixture {
    dir: TempDir,
    bus: TestBus,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    client: ClientId,
    _ui_stream: UnixStream,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = nova_test_support::TempDir::new(&format!("notifications-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let id = hub.register(queue);
        hub.subscribe(id, nova_notifications_api::TOPIC);
        Self { dir, bus, hub, logger, ui, client: id, _ui_stream: ui_stream }
    }

    /// Puts icon `name` where the server finds icons, and returns its file.
    fn install_icon(&self, name: &str) -> String {
        let path = self.dir.join("share/pixmaps").join(format!("{name}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        path.display().to_string()
    }

    fn server(&self) -> Arc<Notifications> {
        self.server_with(None)
    }

    /// A server naming apps from `apps`, or none.
    fn server_with(&self, apps: Option<Arc<AppIndex>>) -> Arc<Notifications> {
        let sources = Sources {
            bus: Bus::Address(self.bus.address.clone()),
            state_file: self.dir.join("notifications.json"),
            images: self.dir.join("images"),
            icons: Arc::new(Icons::new(nova_icons::HICOLOR.into(), &[self.dir.join("share")], None)),
        };
        let apps = apps.unwrap_or_else(|| AppIndex::of(None, &self.logger));
        Notifications::start(sources, apps, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    fn app(&self) -> App {
        let connection = self.bus.connect();
        let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(BUS_NAME).build().unwrap();
        let mut stream = zbus::block_on(MessageStream::for_match_rule(rule, &connection, None)).unwrap();
        let (sender, signals) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Some(Ok(message)) = zbus::block_on(poll_fn(|cx| Pin::new(&mut stream).poll_next(cx))) {
                let member = message.header().member().map(|member| member.to_string()).unwrap_or_default();
                let body = match member.as_str() {
                    "ActionInvoked" => message.body().deserialize::<(u32, String)>().map(|(id, key)| json!([id, key])),
                    _ => message.body().deserialize::<(u32, u32)>().map(|(id, why)| json!([id, why])),
                };
                if sender.send((member, body.unwrap())).is_err() {
                    return;
                }
            }
        });
        App { connection, signals }
    }

    /// An index of the apps installed under `share/applications`
    /// (`entries`: file name and contents); lookups wait for its first read.
    fn apps(&self, entries: &[(&str, &str)]) -> Arc<AppIndex> {
        let share = self.dir.join("share");
        std::fs::create_dir_all(share.join("applications")).unwrap();
        for (file, entry) in entries {
            std::fs::write(
                share.join("applications").join(file),
                format!("[Desktop Entry]\nType=Application\n{entry}"),
            )
            .unwrap();
        }
        AppIndex::of(Some(&share), &self.logger)
    }

    /// The next event called `name`, skipping others.
    fn next(&self, name: &str) -> Value {
        nova_hub::next_named(&self.ui, name, WAIT)
    }

    /// Who owns the server's name on the bus, if anyone.
    fn name_owner(&self) -> String {
        let connection = self.bus.connect();
        let reply = zbus::block_on(connection.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &BUS_NAME,
        ));
        reply.and_then(|reply| reply.body().deserialize::<String>()).unwrap_or_default()
    }

    /// No event called `name` is waiting.
    fn none_waiting(&self, name: &str) {
        while let Ok(Outgoing::Line(line)) = self.ui.try_recv() {
            let event: Value = serde_json::from_str(&line).unwrap();
            assert_ne!(event["event"], name, "{event}");
        }
    }
}

/// `Notify`'s arguments: app name, the id it replaces, app icon, summary,
/// body, actions, hints and timeout.
type Notify<'a> = (&'a str, u32, &'a str, &'a str, &'a str, Vec<&'a str>, HashMap<&'a str, Variant<'a>>, i32);

/// An app on the bus, hearing the server's signals.
struct App {
    connection: Connection,
    signals: Receiver<(String, Value)>,
}

impl App {
    fn notify(&self, notification: Notify<'_>) -> u32 {
        let reply =
            zbus::block_on(self.connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "Notify", &notification))
                .unwrap();
        reply.body().deserialize().unwrap()
    }

    /// A plain notification from `app`.
    fn simple(&self, app: &str, summary: &str) -> u32 {
        self.notify((app, 0, "", summary, "", vec![], HashMap::new(), -1))
    }

    fn signal(&self) -> (String, Value) {
        self.signals.recv_timeout(WAIT).expect("a signal")
    }
}

#[test]
fn a_notification_is_listed_and_pops_up_with_its_text_made_plain() {
    let fixture = Fixture::new("plain");
    let icon = fixture.install_icon("firefox");
    let _server = fixture.server();
    let app = fixture.app();
    let hints = HashMap::from([
        ("desktop-entry", Variant::from("org.mozilla.firefox")),
        ("urgency", Variant::U8(2)),
        ("value", Variant::I32(140)),
    ]);
    let id = app.notify((
        "Firefox",
        0,
        "firefox",
        "Download\ncomplete",
        "<b>niri.tar.gz</b> &amp; <div>",
        vec!["default", "Open", "show", "Show in folder", "", "no key"],
        hints,
        3000,
    ));
    assert_ne!(id, 0);
    let list = fixture.next("notifications");
    let popup = fixture.next("notification_popup");
    let time = popup["time"].clone();
    assert_eq!(
        popup,
        json!({
            "id": id, "app": "Firefox", "icon": icon, "app_id": "org.mozilla.firefox.desktop",
            "summary": "Download complete", "body": "niri.tar.gz & <div>",
            "actions": [{"key": "show", "label": "Show in folder"}], "has_default": true,
            "urgency": "critical", "progress": 100, "timeout_ms": 0, "time": time, "unread": true, "live": true,
        })
    );
    assert_eq!(list["items"], json!([popup]));
    assert_eq!(list["apps"], json!([{"name": "Firefox", "icon": icon, "app_id": "org.mozilla.firefox.desktop"}]));

    let reply =
        zbus::block_on(app.connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "GetCapabilities", &()))
            .unwrap();
    let capabilities: Vec<String> = reply.body().deserialize().unwrap();
    assert!(capabilities.contains(&"actions".to_string()) && capabilities.contains(&"persistence".to_string()));
}

#[test]
fn replacing_keeps_the_id_and_moves_it_to_the_top() {
    let fixture = Fixture::new("replace");
    let _server = fixture.server();
    let app = fixture.app();
    let first = app.simple("Steam", "Downloading");
    let second = app.simple("Discord", "Mara");
    let replaced = app.notify(("Steam", first, "", "Downloaded", "", vec![], HashMap::new(), -1));
    assert_eq!(replaced, first);
    // An id that isn't there any more gets a new one.
    let fresh = app.notify(("Steam", 999, "", "Again", "", vec![], HashMap::new(), -1));
    assert!(![0, 999, first, second].contains(&fresh));
    let mut list = fixture.next("notifications");
    while list["items"].as_array().unwrap().len() < 3 {
        list = fixture.next("notifications");
    }
    let items: Vec<_> =
        list["items"].as_array().unwrap().iter().map(|item| (item["id"].clone(), item["summary"].clone())).collect();
    assert_eq!(
        items,
        [(json!(fresh), json!("Again")), (json!(first), json!("Downloaded")), (json!(second), json!("Mara"))]
    );
}

#[test]
fn muted_apps_are_hidden_and_do_not_disturb_holds_back_pop_ups() {
    let fixture = Fixture::new("quiet");
    let server = fixture.server();
    let app = fixture.app();
    let first = app.simple("Discord", "first");
    fixture.next("notification_popup");
    // Only an app that sent one can be muted.
    assert!(server.mute("Slack", true).is_err());
    server.mute("Discord", true).unwrap();
    let list = fixture.next("notifications");
    assert_eq!(list["muted"], json!(["Discord"]));
    assert_eq!(list["items"], json!([]), "muting hides its notifications");

    // New ones aren't kept, and clearing leaves the hidden ones.
    app.simple("Discord", "muted");
    server.clear(None);
    server.mute("Discord", false).unwrap();
    let list = fixture.next("notifications");
    assert_eq!(list["muted"], json!([]));
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list["items"][0]["id"], first, "unmuting shows them again");
    assert_eq!(list["items"][0].get("unread"), None, "muting read them");
    fixture.none_waiting("notification_popup");

    server.set_do_not_disturb(true);
    assert_eq!(fixture.next("notifications")["do_not_disturb"], true);
    app.simple("Firefox", "held back");
    let list = fixture.next("notifications");
    assert_eq!(list["items"][0]["unread"], true, "still unread during do not disturb");
    fixture.none_waiting("notification_popup");
    let critical = HashMap::from([("urgency", Variant::U8(2))]);
    app.notify(("Firefox", 0, "", "battery low", "", vec![], critical, -1));
    assert_eq!(fixture.next("notification_popup")["summary"], "battery low");

    server.read(None);
    let list = fixture.next("notifications");
    assert!(list["items"].as_array().unwrap().iter().all(|item| item.get("unread").is_none()));
}

#[test]
fn one_notification_can_be_read() {
    let fixture = Fixture::new("read-one");
    let server = fixture.server();
    let app = fixture.app();
    let first = app.simple("Discord", "first");
    app.simple("Discord", "second");
    let list = fixture.next("notifications");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    fixture.next("notifications");
    server.read(Some(first));
    let list = fixture.next("notifications");
    assert_eq!(list["items"][0]["unread"], true);
    assert_eq!(list["items"][1]["id"], first);
    assert_eq!(list["items"][1].get("unread"), None);
}

#[test]
fn clicks_reach_the_app_and_keep_the_notification_read() {
    let fixture = Fixture::new("click");
    let server = fixture.server();
    let app = fixture.app();
    let actions = vec!["default", "Open", "reply", "Reply"];
    let id = app.notify(("Chat", 0, "", "Mara", "hi", actions.clone(), HashMap::new(), -1));
    assert!(server.action(id, Some("unknown")).is_err());
    server.action(id, Some("reply")).unwrap();
    assert_eq!(app.signal(), ("ActionInvoked".into(), json!([id, "reply"])));
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([id, 2])));
    let list = loop {
        let list = fixture.next("notifications");
        if list["items"][0].get("unread").is_none() {
            break list;
        }
    };
    assert_eq!(list["items"][0]["id"], id, "still listed");
    assert_eq!(list["items"][0].get("live"), None, "its app is done with it");
    assert!(server.action(id, None).is_err(), "a click on it again opens the app instead (`click`)");
    // Many apps close it on a click too: it stays.
    zbus::block_on(app.connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "CloseNotification", &(id,)))
        .unwrap();
    app.simple("Mail", "later");
    let list = loop {
        let list = fixture.next("notifications");
        if list["items"][0]["summary"] == "later" {
            break list;
        }
    };
    assert_eq!(list["items"][1]["id"], id, "closing it again leaves it listed");

    // A resident one stays when a button is clicked, and goes on a click.
    let resident = HashMap::from([("resident", Variant::Bool(true))]);
    let id = app.notify(("Player", 0, "", "Song", "", actions, resident, -1));
    server.action(id, Some("reply")).unwrap();
    assert_eq!(app.signal(), ("ActionInvoked".into(), json!([id, "reply"])));
    server.action(id, None).unwrap();
    assert_eq!(app.signal(), ("ActionInvoked".into(), json!([id, "default"])));
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([id, 2])));
}

#[test]
fn closing_dismissing_and_clearing_tell_the_app_why() {
    let fixture = Fixture::new("close");
    let server = fixture.server();
    let app = fixture.app();
    let first = app.simple("Chat", "one");
    let second = app.simple("Chat", "two");
    let other = app.simple("Mail", "three");
    zbus::block_on(app.connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "CloseNotification", &(first,)))
        .unwrap();
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([first, 3])));
    server.dismiss(second);
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([second, 2])));
    server.clear(Some("Chat"));
    server.clear(None);
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([other, 2])));
    let mut list = fixture.next("notifications");
    while !list["items"].as_array().unwrap().is_empty() {
        list = fixture.next("notifications");
    }
}

// Chat apps (a web notification's tag in Electron) close the last
// message's notification as they send the next one: both stay listed.
#[test]
fn an_app_closing_its_notification_leaves_it_listed() {
    let fixture = Fixture::new("closed-by-app");
    let _server = fixture.server();
    let app = fixture.app();
    let first = app.simple("Ferdium", "one");
    zbus::block_on(app.connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "CloseNotification", &(first,)))
        .unwrap();
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([first, 3])));
    let second = app.simple("Ferdium", "two");
    let list = loop {
        let list = fixture.next("notifications");
        if list["items"][0]["id"] == second {
            break list;
        }
    };
    assert_eq!(list["items"].as_array().unwrap().len(), 2, "{list}");
    assert_eq!(list["items"][1]["id"], first);
    assert_eq!(list["items"][1]["unread"], true, "not seen yet");
    assert_eq!(list["items"][1].get("live"), None, "its app is done with it");
}

#[test]
fn transient_notifications_only_pop_up() {
    let fixture = Fixture::new("transient");
    let server = fixture.server();
    let app = fixture.app();
    let transient = HashMap::from([("transient", Variant::Bool(true))]);
    let id = app.notify(("Volume", 0, "", "50%", "", vec!["default", "Open"], transient, -1));
    let list = fixture.next("notifications");
    assert_eq!(fixture.next("notification_popup")["id"], id);
    assert_eq!(list["items"], json!([]));
    assert_eq!(list["apps"][0]["name"], "Volume");
    assert_eq!(list["transient"][0]["id"], id, "listed apart, so its pop-up knows it's open");
    // Its pop-up's click still reaches the app.
    server.action(id, None).unwrap();
    assert_eq!(app.signal(), ("ActionInvoked".into(), json!([id, "default"])));
    assert_eq!(fixture.next("notifications").get("transient"), None, "gone, so its pop-up goes");

    // Closed by its app, it goes too.
    let transient = HashMap::from([("transient", Variant::Bool(true))]);
    let id = app.notify(("Volume", 0, "", "60%", "", vec![], transient, 0));
    assert_eq!(fixture.next("notifications")["transient"][0]["id"], id);
    zbus::block_on(app.connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "CloseNotification", &(id,)))
        .unwrap();
    assert_eq!(fixture.next("notifications").get("transient"), None);
}

#[test]
fn pictures_are_written_out_and_deleted_with_their_notification() {
    let fixture = Fixture::new("image");
    let server = fixture.server();
    let app = fixture.app();
    let pixels = (2i32, 1i32, 8i32, true, 8i32, 4i32, vec![255u8, 0, 0, 255, 0, 255, 0, 255]);
    let hints = HashMap::from([("image-data", Variant::from(zbus::wire::Structure::from(pixels.clone())))]);
    let id = app.notify(("Chat", 0, "", "Mara", "", vec![], hints, -1));
    let image = image_of(&server, id);
    assert_eq!(image.parent(), Some(fixture.dir.join("images").as_path()));
    assert_eq!(fixture.next("notification_popup")["image"], image.display().to_string());
    assert!(image.exists());
    // New pixels get a path of their own, so the UI doesn't show the old ones.
    let hints = HashMap::from([("image-data", Variant::from(zbus::wire::Structure::from(pixels.clone())))]);
    app.notify(("Chat", id, "", "Mara", "", vec![], hints, -1));
    fixture.next("notification_popup");
    let replaced = image_of(&server, id);
    assert!(replaced != image && replaced.exists() && !image.exists());
    server.dismiss(id);
    assert!(!replaced.exists());

    let hints = HashMap::from([("image-path", Variant::from("file:///tmp/mara.png"))]);
    app.notify(("Chat", 0, "", "Mara", "", vec![], hints, -1));
    assert_eq!(fixture.next("notification_popup")["image"], "/tmp/mara.png");

    // A reference too long to be a path is dropped, as an image or an app icon.
    let long = format!("/{}", "a".repeat(4 * 1024 * 1024));
    let hints = HashMap::from([("image-path", Variant::from(long.as_str()))]);
    app.notify(("Chat", 0, &long, "Mara", "", vec![], hints, -1));
    let popup = fixture.next("notification_popup");
    assert!(popup.get("image").is_none() && popup.get("icon").is_none(), "{}", popup.to_string().len());

    // A file an app names is never deleted, even one that reads as ours.
    let victim = fixture.dir.join("victim");
    std::fs::write(&victim, "keep").unwrap();
    let named = format!("{}/../victim", fixture.dir.join("images").display());
    let hints = HashMap::from([("image-path", Variant::from(named.as_str()))]);
    let id = app.notify(("Chat", 0, "", "Mara", "", vec![], hints, -1));
    server.dismiss(id);
    assert!(victim.exists());

    // Replaced by one without a picture, its picture goes.
    let hints = HashMap::from([("image-data", Variant::from(zbus::wire::Structure::from(pixels.clone())))]);
    let id = app.notify(("Chat", 0, "", "Mara", "", vec![], hints, -1));
    let image = image_of(&server, id);
    assert!(image.exists());
    app.notify(("Chat", id, "", "Mara", "", vec![], HashMap::new(), -1));
    assert!(!image.exists());

    // So do transient ones' pictures, once newer ones push them out.
    let transient = || {
        HashMap::from([
            ("transient", Variant::Bool(true)),
            ("image-data", Variant::from(zbus::wire::Structure::from(pixels.clone()))),
        ])
    };
    let first = app.notify(("Volume", 0, "", "50%", "", vec![], transient(), -1));
    let image = image_of(&server, first);
    assert!(image.exists());
    for _ in 0..MAX_TRANSIENT {
        app.notify(("Volume", 0, "", "60%", "", vec![], transient(), -1));
    }
    assert!(!image.exists());
}

/// The picture novad has for notification `id`.
fn image_of(server: &Notifications, id: u32) -> PathBuf {
    let state = lock(&server.shared.state);
    PathBuf::from(find(&state, id).and_then(|kept| kept.image.clone()).expect("a picture"))
}

#[test]
fn pictures_no_notification_keeps_are_deleted_at_start() {
    let fixture = Fixture::new("stray");
    let images = fixture.dir.join("images");
    std::fs::create_dir_all(&images).unwrap();
    let kept = images.join("2-1.png");
    let stray = images.join("1-1.png");
    std::fs::write(&kept, "png").unwrap();
    std::fs::write(&stray, "png").unwrap();
    let now = jiff::Timestamp::now().as_second();
    let item =
        json!({"id": 2, "app": "Chat", "summary": "hi", "body": "", "image": kept, "urgency": "normal", "time": now});
    std::fs::write(fixture.dir.join("notifications.json"), json!({"next_id": 2, "items": [item]}).to_string()).unwrap();
    let _server = fixture.server();
    assert!(kept.exists() && !stray.exists());
}

#[test]
fn an_app_that_quits_leaves_its_notifications_unclickable() {
    let fixture = Fixture::new("quit");
    let server = fixture.server();
    // Like notify-send: connects, sends one and leaves.
    let connection = fixture.bus.connect();
    let sent: Notify<'_> = ("Chat", 0, "", "Mara", "", vec!["default", "Open"], HashMap::new(), -1);
    let reply = zbus::block_on(connection.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "Notify", &sent)).unwrap();
    let id: u32 = reply.body().deserialize().unwrap();
    assert_eq!(fixture.next("notification_popup")["live"], true);
    drop(connection);
    let list = loop {
        let list = fixture.next("notifications");
        if list["items"][0].get("live").is_none() {
            break list;
        }
    };
    assert_eq!(list["items"][0]["id"], id);
    assert!(server.action(id, None).is_err());
    let clicked = server.click(id).unwrap();
    assert_eq!(clicked, Clicked { app_id: None, heard: false }, "novad opens the app instead");
    let list = fixture.next("notifications");
    assert_eq!(list["items"][0].get("unread"), None, "read all the same");
}

#[test]
fn an_installed_desktop_entry_names_the_app_before_the_sender_does() {
    let fixture = Fixture::new("desktop-entry-name");
    let patched = fixture.install_icon("discord-patched");
    let theme = fixture.install_icon("vesktop");
    let apps = fixture.apps(&[("discord.desktop", "Name=Discord\nIcon=discord-patched\nExec=true\n")]);
    let _server = fixture.server_with(Some(apps));
    let app = fixture.app();

    // The entry the sender names is installed: its name and icon win over
    // what the sender calls itself and the icon it gives.
    let hints = HashMap::from([("desktop-entry", Variant::from("discord"))]);
    app.notify(("discord-canary", 0, "vesktop", "Mara", "hi", vec![], hints, -1));
    let popup = fixture.next("notification_popup");
    assert_eq!((&popup["app"], &popup["icon"]), (&json!("Discord"), &json!(patched)));
    assert_eq!(popup["app_id"], "discord.desktop");

    // Not installed: the sender's own name and icon.
    let hints = HashMap::from([("desktop-entry", Variant::from("vesktop"))]);
    app.notify(("Vesktop", 0, "vesktop", "Mara", "hi", vec![], hints, -1));
    let popup = fixture.next("notification_popup");
    assert_eq!((&popup["app"], &popup["icon"]), (&json!("Vesktop"), &json!(theme)));

    // No entry named: the sender's name.
    app.simple("Chat", "hello");
    assert_eq!(fixture.next("notification_popup")["app"], "Chat");
}

#[test]
fn a_click_says_which_app_sent_it_and_whether_the_app_heard() {
    let fixture = Fixture::new("click-app");
    let server = fixture.server();
    let app = fixture.app();
    let hints = HashMap::from([("desktop-entry", Variant::from("discord"))]);
    let id = app.notify(("Discord", 0, "", "Mara", "hi", vec!["default", "Open"], hints.clone(), -1));
    let discord = Some("discord.desktop".to_string());
    assert_eq!(server.click(id).unwrap(), Clicked { app_id: discord.clone(), heard: true });
    assert_eq!(app.signal(), ("ActionInvoked".into(), json!([id, "default"])));
    assert_eq!(server.click(id).unwrap(), Clicked { app_id: discord.clone(), heard: false }, "done with it");
    let plain = app.notify(("Discord", 0, "", "Mara", "hi", vec![], hints, -1));
    assert_eq!(server.click(plain).unwrap(), Clicked { app_id: discord, heard: false }, "does nothing on a click");
    assert!(server.click(999).unwrap_err().contains("no notification 999"));
}

#[test]
fn the_list_and_settings_survive_a_restart() {
    let fixture = Fixture::new("restart");
    let server = fixture.server();
    let app = fixture.app();
    let id = app.simple("Chat", "kept");
    server.set_do_not_disturb(true);
    server.stop();
    // Let go of the name before the next one claims it: the bus frees it
    // only once the old connection has closed, after `serve` returns, and
    // an app calling before then reaches the closing server and gets no reply.
    wait_until(WAIT, "the server to let go of its name", || fixture.name_owner().is_empty());
    drop(server);
    while fixture.ui.try_recv().is_ok() {}

    let server = fixture.server();
    server.send_state(fixture.client);
    let list = loop {
        let list = fixture.next("notifications");
        if list["do_not_disturb"] == true {
            break list;
        }
    };
    assert_eq!(list["items"][0]["id"], id);
    assert_eq!(list["items"][0].get("live"), None, "its app isn't there to hear clicks");
    assert_ne!(app.simple("Chat", "new"), id, "ids carry on");
}

#[test]
fn the_list_keeps_the_newest_and_tells_apps_about_the_rest() {
    let fixture = Fixture::new("full");
    let _server = fixture.server();
    let app = fixture.app();
    let first = app.simple("Chat", "oldest");
    for _ in 0..MAX_NOTIFICATIONS {
        app.simple("Chat", "newer");
    }
    assert_eq!(app.signal(), ("NotificationClosed".into(), json!([first, Closed::Other as u32])));
}

#[test]
fn notifications_older_than_a_week_are_dropped_at_start() {
    let fixture = Fixture::new("old");
    let now = jiff::Timestamp::now().as_second();
    let kept = |id: u32, time: i64| json!({"id": id, "app": "Chat", "summary": "hi", "body": "", "urgency": "normal", "time": time});
    let saved = json!({"next_id": 2, "items": [kept(2, now - 60), kept(1, now - KEEP_SECS - 60)]});
    std::fs::write(fixture.dir.join("notifications.json"), saved.to_string()).unwrap();
    let server = fixture.server();
    server.send_state(fixture.client);
    let items = fixture.next("notifications")["items"].clone();
    assert_eq!(items.as_array().unwrap().iter().map(|item| item["id"].clone()).collect::<Vec<_>>(), [json!(2)]);
}

#[test]
fn icons_kept_by_name_are_looked_up_at_start() {
    let fixture = Fixture::new("icon-names");
    let chat = fixture.install_icon("chat");
    let now = jiff::Timestamp::now().as_second();
    let kept = |id: u32, icon: &str| json!({"id": id, "app": "Chat", "icon": icon, "summary": "hi", "body": "", "urgency": "normal", "time": now});
    let saved = json!({
        "next_id": 2,
        "items": [kept(2, "chat"), kept(1, "not-installed")],
        "apps": [{"name": "Chat", "icon": "chat"}],
    });
    std::fs::write(fixture.dir.join("notifications.json"), saved.to_string()).unwrap();
    let server = fixture.server();
    server.send_state(fixture.client);
    let list = fixture.next("notifications");
    assert_eq!(list["items"][0]["icon"], chat);
    assert_eq!(list["items"][1].get("icon"), None);
    assert_eq!(list["apps"], json!([{"name": "Chat", "icon": chat}]));
}

#[test]
fn quick_updates_are_saved_at_most_once_a_second_ending_with_the_last() {
    use std::os::unix::fs::MetadataExt;
    let fixture = Fixture::new("save-rate");
    let _server = fixture.server();
    let app = fixture.app();
    let file = fixture.dir.join("notifications.json");
    let saved = |summary: &str| std::fs::read_to_string(&file).is_ok_and(|text| text.contains(summary));
    let id = app.simple("Firefox", "step 0");
    wait_until(WAIT, "the first change to be saved at once", || saved("step 0"));
    // A download's progress, every 50 ms for 1.5 s; every write seen.
    let mut writes = HashSet::new();
    let updates = 30;
    for step in 1..=updates {
        app.notify(("Firefox", id, "", &format!("step {step}"), "", vec![], HashMap::new(), -1));
        if let Ok(meta) = std::fs::metadata(&file) {
            writes.insert((meta.ino(), meta.mtime_nsec()));
        }
        nova_test_support::settle(Duration::from_millis(50));
    }
    let last = format!("step {updates}");
    wait_until(2 * SAVE_EVERY + WAIT, "the last change to be saved", || saved(&last));
    assert!(writes.len() <= 4, "{} writes for {updates} updates in 1.5 s", writes.len());
}

#[test]
fn nothing_takes_the_name_and_a_queued_server_takes_over() {
    let fixture = Fixture::new("queued");
    let first = fixture.server();
    let other = fixture.bus.connect();
    let replacing =
        zbus::block_on(other.request_name_with_flags(BUS_NAME, zbus::fdo::RequestNameFlags::REPLACE_EXISTING));
    assert_eq!(replacing.unwrap(), RequestNameReply::InQueue, "the running server keeps the name");
    drop(other);
    let owner = |server: &Notifications| {
        let connection = lock(&server.shared.connection).clone().expect("serving");
        connection.unique_name().expect("on a bus").to_string()
    };
    assert_eq!(fixture.name_owner(), owner(&first));

    let second = fixture.server();
    assert_eq!(fixture.name_owner(), owner(&first), "the second one waits");
    first.stop();
    wait_until(WAIT, "the second server to take over", || fixture.name_owner() == owner(&second));
}
