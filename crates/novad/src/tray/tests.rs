//! The tray against a real bus: a private `dbus-daemon` per test, with apps
//! that put up items the way Electron (an object path on its own
//! connection) and libayatana (a bus name of its own) do.

use super::*;
use crate::apps::AppIndex;
use crate::hub::{ClientQueue, Outgoing};
use crate::session_bus::TestBus;
use nova_protocol::topics;
use nova_test_support::{TempDir, eventually, wait_until};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;

/// How long a test waits for something the bus should do at once. Longer
/// than reading an item may take (three calls, each up to its timeout), so
/// a busy test machine doesn't fail it.
const WAIT: Duration = Duration::from_secs(4 * CALL_TIMEOUT.as_secs());

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
        let dir = nova_test_support::TempDir::new(&format!("tray-{name}"));
        std::fs::create_dir_all(dir.join("proc")).unwrap();
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let id = hub.register(queue);
        hub.subscribe(id, topics::TRAY);
        Self { dir, bus, hub, logger, ui, _ui_stream: ui_stream }
    }

    /// A tray naming apps from `apps`, or none.
    fn tray(&self, apps: Option<Arc<AppIndex>>) -> Arc<Tray> {
        let sources = Sources {
            bus: Bus::Address(self.bus.address.clone()),
            state_file: self.dir.join("tray.json"),
            proc_root: self.dir.join("proc"),
        };
        let apps = apps.unwrap_or_else(|| AppIndex::of(None, &self.logger));
        Tray::start(sources, apps, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    fn connect(&self) -> Connection {
        self.bus.connect()
    }

    /// The next tray event's items.
    fn next_items(&self) -> Value {
        crate::hub::next_named(&self.ui, "tray", WAIT)["items"].clone()
    }

    /// Makes our process look, to the tray, like it runs in `unit`.
    fn run_in_unit(&self, unit: &str) {
        let process = self.dir.join("proc").join(std::process::id().to_string());
        std::fs::create_dir_all(&process).unwrap();
        std::fs::write(process.join("cgroup"), format!("0::/user.slice/app.slice/{unit}\n")).unwrap();
    }
}

/// An app's tray item, with the two properties the tray reads.
pub struct Item {
    pub id: String,
    pub title: String,
    /// Counts `Activate` calls.
    pub activated: Arc<std::sync::atomic::AtomicUsize>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }

    fn activate(&self, _x: i32, _y: i32) {
        self.activated.fetch_add(1, Ordering::SeqCst);
    }
}

/// Puts up an item at `path` on a new connection, owning `name` first if
/// given, and registers `service` with the watcher.
fn put_up(fixture: &Fixture, name: Option<&str>, path: &str, service: &str, id: &str) -> Connection {
    put_up_counting(fixture, name, path, service, id, Arc::default())
}

/// Like `put_up`, counting the item's `Activate` calls in `activated`.
fn put_up_counting(
    fixture: &Fixture,
    name: Option<&str>,
    path: &str,
    service: &str,
    id: &str,
    activated: Arc<std::sync::atomic::AtomicUsize>,
) -> Connection {
    let connection = fixture.connect();
    serve_item(&connection, name, path, service, Item { id: id.into(), title: format!("{id} title"), activated });
    connection
}

/// Serves `item` at `path` on `connection`, owning `name` first if given,
/// and registers `service` with the watcher.
pub fn serve_item(
    connection: &Connection,
    name: Option<&str>,
    path: &str,
    service: &str,
    item: impl zbus::object_server::Interface,
) {
    zbus::block_on(async {
        connection.object_server().at(path, item).await.unwrap();
        if let Some(name) = name {
            connection.request_name(name).await.unwrap();
        }
        register(connection, service).await.unwrap();
    });
}

async fn register(connection: &Connection, service: &str) -> zbus::Result<zbus::Message> {
    let method = "RegisterStatusNotifierItem";
    connection.call_method(Some(WATCHER_NAME), WATCHER_PATH, Some(WATCHER_NAME), method, &(service,)).await
}

fn watcher_property<T: TryFrom<OwnedVariant>>(connection: &Connection, name: &str) -> T
where
    T::Error: std::fmt::Debug,
{
    zbus::block_on(async {
        let reply = connection
            .call_method(Some(WATCHER_NAME), WATCHER_PATH, Some(PROPERTIES), "Get", &(WATCHER_NAME, name))
            .await
            .unwrap();
        reply.body().deserialize::<OwnedVariant>().unwrap().try_into().unwrap()
    })
}

#[test]
fn an_electron_item_shows_with_its_app_and_goes_with_it() {
    let fixture = Fixture::new("electron");
    let apps = fixture.dir.join("share/applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("discord.desktop"), "[Desktop Entry]\nType=Application\nName=Discord\nExec=true\n")
        .unwrap();
    let apps = AppIndex::of(Some(&fixture.dir.join("share")), &fixture.logger);
    wait_until(WAIT, "the index to find discord.desktop", || apps.find_app(&["discord".into()]).is_some());
    // Electron's own id doesn't name the app; the unit it runs in does.
    fixture.run_in_unit("app-discord-375306.scope");
    let tray = fixture.tray(Some(Arc::clone(&apps)));

    let activated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let item_id = "chrome_status_icon_1";
    let app = put_up_counting(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", item_id, activated.clone());
    let address = format!("{}/StatusNotifierItem", app.unique_name().unwrap());
    let items = fixture.next_items();
    assert_eq!(items[0]["address"], json!(address));
    assert_eq!(items[0]["id"], "chrome_status_icon_1");
    assert_eq!(items[0]["title"], "chrome_status_icon_1 title");
    assert_eq!(items[0]["app"]["id"], "discord.desktop");
    let host = fixture.connect();
    assert_eq!(watcher_property::<Vec<String>>(&host, "RegisteredStatusNotifierItems"), std::slice::from_ref(&address));

    // The launcher reopens an app that only sits in the tray through its item.
    assert_eq!(tray.activate_app("discord.desktop"), Ok(true));
    assert_eq!(activated.load(Ordering::SeqCst), 1);
    assert_eq!(tray.activate_app("steam.desktop"), Ok(false));

    drop(app);
    assert_eq!(fixture.next_items(), json!([]));
    assert!(watcher_property::<Vec<String>>(&host, "RegisteredStatusNotifierItems").is_empty());
    tray.stop();
    apps.forget();
}

#[test]
fn a_bus_name_item_gets_the_default_path() {
    let fixture = Fixture::new("bus-name");
    let _tray = fixture.tray(None);
    let name = "org.kde.StatusNotifierItem-4242-1";
    let _app = put_up(&fixture, Some(name), "/StatusNotifierItem", name, "steam");
    let items = fixture.next_items();
    assert_eq!(items[0]["address"], json!(format!("{name}/StatusNotifierItem")));
    assert_eq!(items[0]["id"], "steam");
    assert!(items[0].get("app").is_none());
}

#[test]
fn a_host_is_always_there_and_bad_registrations_are_refused() {
    let fixture = Fixture::new("refused");
    let _tray = fixture.tray(None);
    let app = fixture.connect();
    // Chromium checks this once, before any UI may have started.
    assert!(watcher_property::<bool>(&app, "IsStatusNotifierHostRegistered"));
    for service in ["not a name", "org.nobody.Owns.This", "/bad//path", &"a".repeat(MAX_SERVICE_LEN + 1)] {
        assert!(zbus::block_on(register(&app, service)).is_err(), "{service} is refused");
    }
    assert!(watcher_property::<Vec<String>>(&app, "RegisteredStatusNotifierItems").is_empty());
}

#[test]
fn nothing_takes_the_name_and_a_queued_tray_takes_over_when_the_first_stops() {
    let fixture = Fixture::new("queued");
    let first = fixture.tray(None);
    let other = fixture.connect();
    let replacing =
        zbus::block_on(other.request_name_with_flags(WATCHER_NAME, zbus::fdo::RequestNameFlags::REPLACE_EXISTING));
    assert_eq!(replacing.unwrap(), RequestNameReply::InQueue, "the running tray keeps the name");
    drop(other);
    let second = fixture.tray(None);
    first.stop();
    // Until the bus has given the name to the second tray.
    let app = eventually(WAIT, "the second tray to take over", || put_up_retrying(&fixture));
    assert_eq!(fixture.next_items()[0]["id"], "late");
    drop(app);
    second.stop();
}

/// Puts up an item if the watcher it reaches is one that's running.
fn put_up_retrying(fixture: &Fixture) -> Option<Connection> {
    let connection = fixture.connect();
    zbus::block_on(async {
        let item = Item { id: "late".into(), title: String::new(), activated: Arc::default() };
        connection.object_server().at("/StatusNotifierItem", item).await.unwrap();
        register(&connection, "/StatusNotifierItem").await.ok()?;
        Some(())
    })?;
    Some(connection)
}

#[test]
fn hidden_apps_are_remembered_across_restarts() {
    let fixture = Fixture::new("hidden");
    let tray = fixture.tray(None);
    assert!(tray.hide("steam", true).is_err(), "only an app in the tray can be hidden");
    let _app = put_up(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", "steam");
    fixture.next_items();
    tray.hide("steam", true).unwrap();
    let event = |fixture: &Fixture| loop {
        if let Outgoing::Line(line) = fixture.ui.recv_timeout(WAIT).expect("a tray event") {
            let event: Value = serde_json::from_str(&line).unwrap();
            if event["event"] == "tray" {
                return event["data"].clone();
            }
        }
    };
    assert_eq!(event(&fixture)["hidden"], json!(["steam"]));
    tray.stop();

    // A new tray (novad restarted) still hides it, with the app gone meanwhile.
    let tray = fixture.tray(None);
    tray.hide("steam", false).unwrap();
    assert!(event(&fixture).get("hidden").is_none());
    tray.stop();
}

/// A libayatana item, like Spotify's: no `Activate`, only a menu.
pub struct MenuOnlyItem {
    pub id: String,
    pub title: String,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl MenuOnlyItem {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {}
}

#[test]
fn an_item_with_only_a_menu_is_never_activated() {
    let fixture = Fixture::new("menu-only");
    let apps = fixture.dir.join("share/applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("spotify.desktop"), "[Desktop Entry]\nType=Application\nName=Spotify\nExec=true\n")
        .unwrap();
    let apps = AppIndex::of(Some(&fixture.dir.join("share")), &fixture.logger);
    wait_until(WAIT, "the index to find spotify.desktop", || apps.find_app(&["spotify".into()]).is_some());
    fixture.run_in_unit("app-spotify-277486.scope");
    let tray = fixture.tray(Some(apps));

    let path = "/org/ayatana/NotificationItem/spotify_client";
    let app = fixture.connect();
    serve_item(&app, None, path, path, MenuOnlyItem { id: "spotify-client".into(), title: "spotify".into() });
    let items = fixture.next_items();
    assert_eq!(items[0]["app"]["id"], "spotify.desktop", "{items}");
    assert_eq!(items[0]["menu_only"], true, "introspected, it has no Activate");

    assert!(tray.has_app("spotify.desktop"), "it runs, in the tray");
    assert_eq!(tray.activate_app("spotify.desktop"), Ok(false), "its icon can't show its window");
    let address = items[0]["address"].as_str().unwrap();
    let click = tray.activate(address).unwrap_err();
    assert!(click.contains("only a menu"), "{click}");
    tray.stop();
}

#[test]
fn introspection_says_which_methods_an_interface_has() {
    // As libayatana's item answers (GDBus).
    let ayatana = r#"<node>
  <interface name="org.freedesktop.DBus.Properties"><method name="Get"></method></interface>
  <interface name="org.kde.StatusNotifierItem">
    <method name="Scroll"><arg type="i" name="delta" direction="in"/></method>
    <method name="SecondaryActivate"><arg type="i" name="x" direction="in"/></method>
    <method name="XAyatanaSecondaryActivate"><arg type="u" name="timestamp" direction="in"/></method>
  </interface>
</node>"#;
    assert_eq!(has_method(ayatana, ITEM_INTERFACE, "Activate"), Some(false));
    assert_eq!(has_method(ayatana, ITEM_INTERFACE, "Scroll"), Some(true));
    let kde = "<node><interface name='org.kde.StatusNotifierItem'><method name='Activate'/></interface></node>";
    assert_eq!(has_method(kde, ITEM_INTERFACE, "Activate"), Some(true));
    let other = r#"<node><interface name="org.freedesktop.DBus.Peer"><method name="Activate"/></interface></node>"#;
    assert_eq!(has_method(other, ITEM_INTERFACE, "Activate"), None, "another interface's method isn't the item's");
}

#[test]
fn an_app_with_its_own_item_loses_its_bridged_x11_one() {
    let item = |id: &str, app: Option<&str>| TrayItem {
        address: format!(":1.{id}/StatusNotifierItem"),
        id: id.into(),
        title: id.into(),
        app: app.map(|app| App { id: app.into(), name: app.into(), description: None, icon: None }),
        menu_only: false,
    };
    let items = vec![
        (false, item("steam", Some("steam.desktop"))),
        (true, item("steam", Some("steam.desktop"))),
        (true, item("wine-app", None)),
        (true, item("xterm", Some("xterm.desktop"))),
    ];
    let ids: Vec<String> = without_bridged_twins(items).into_iter().map(|item| item.address).collect();
    assert_eq!(ids, [":1.steam/StatusNotifierItem", ":1.wine-app/StatusNotifierItem", ":1.xterm/StatusNotifierItem"]);
}

#[test]
fn an_app_registers_only_items_on_its_own_connection() {
    let fixture = Fixture::new("not-own");
    let _tray = fixture.tray(None);
    let other = fixture.connect();
    let other_name = other.unique_name().unwrap().to_string();
    let app = fixture.connect();
    // Names the watcher (novad) and another app own, as a name or a unique name.
    for service in [format!("{WATCHER_NAME}/Bogus0"), format!("{other_name}/StatusNotifierItem"), other_name.clone()] {
        assert!(zbus::block_on(register(&app, &service)).is_err(), "{service} is refused");
    }
    assert!(watcher_property::<Vec<String>>(&app, "RegisteredStatusNotifierItems").is_empty());
}

#[test]
fn an_item_that_cant_tell_its_id_is_dropped() {
    let fixture = Fixture::new("no-id");
    let _tray = fixture.tray(None);
    let app = fixture.connect();
    // The app serves objects, but nothing at this path.
    let other = Item { id: "other".into(), title: String::new(), activated: Arc::default() };
    zbus::block_on(app.object_server().at("/Other", other)).unwrap();
    zbus::block_on(register(&app, "/Nothing")).unwrap();
    wait_until(WAIT, "the item without an id to be dropped", || {
        watcher_property::<Vec<String>>(&app, "RegisteredStatusNotifierItems").is_empty()
    });
    // A real item still shows.
    let real = put_up(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", "steam");
    let items = loop {
        // Past the empty list the drop may publish.
        let items = fixture.next_items();
        if items != json!([]) {
            break items;
        }
    };
    assert_eq!(
        items,
        json!([{ "address": format!("{}/StatusNotifierItem", real.unique_name().unwrap()), "id": "steam", "title": "steam title" }])
    );
}

#[test]
fn one_app_cant_fill_the_tray() {
    let fixture = Fixture::new("per-app");
    let _tray = fixture.tray(None);
    let app = fixture.connect();
    for n in 0..MAX_ITEMS_PER_APP {
        let item = Item { id: format!("item{n}"), title: String::new(), activated: Arc::default() };
        serve_item(&app, None, &format!("/Item{n}"), &format!("/Item{n}"), item);
    }
    assert!(zbus::block_on(register(&app, "/OneMore")).is_err(), "past the cap is refused");
    // Another app still gets in.
    let _other = put_up(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", "steam");
    wait_until(WAIT, "the other app's item to be registered", || {
        watcher_property::<Vec<String>>(&app, "RegisteredStatusNotifierItems").len() > MAX_ITEMS_PER_APP
    });
}
