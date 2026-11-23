use super::*;
use crate::test_items::{Item, LookItem, Menu, MenuNode, MenuOnlyItem, register, serve_item};
use nova_hub::{ClientQueue, Outgoing};
use nova_session_bus::TestBus;
use nova_test_support::{TempDir, eventually, wait_until};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;

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
        hub.subscribe(id, nova_tray_api::TOPIC);
        Self { dir, bus, hub, logger, ui, _ui_stream: ui_stream }
    }

    fn tray(&self, apps: Option<Arc<AppIndex>>) -> Arc<Tray> {
        let sources = Sources {
            bus: Bus::Address(self.bus.address.clone()),
            state_file: self.dir.join("tray.json"),
            proc_root: self.dir.join("proc"),
            icons: Arc::new(Icons::new(nova_icons::HICOLOR.into(), &[self.dir.join("share")], None)),
            pictures: self.dir.join("pictures"),
        };
        let apps = apps.unwrap_or_else(|| AppIndex::of(None, &self.logger));
        Tray::start(sources, apps, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    fn connect(&self) -> Connection {
        self.bus.connect()
    }

    fn next_items(&self) -> Value {
        nova_hub::next_named(&self.ui, "tray", WAIT)["items"].clone()
    }

    fn run_in_unit(&self, unit: &str) {
        let process = self.dir.join("proc").join(std::process::id().to_string());
        std::fs::create_dir_all(&process).unwrap();
        std::fs::write(process.join("cgroup"), format!("0::/user.slice/app.slice/{unit}\n")).unwrap();
    }
}

fn put_up(fixture: &Fixture, name: Option<&str>, path: &str, service: &str, id: &str) -> Connection {
    put_up_item(fixture, name, path, service, item(id, Arc::default()))
}

fn put_up_item(fixture: &Fixture, name: Option<&str>, path: &str, service: &str, item: Item) -> Connection {
    let connection = fixture.connect();
    serve_item(&connection, name, path, service, item);
    connection
}

fn item(id: &str, activated: Arc<std::sync::atomic::AtomicUsize>) -> Item {
    Item { id: id.into(), title: format!("{id} title"), activated }
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
    fixture.run_in_unit("app-discord-375306.scope");
    let tray = fixture.tray(Some(Arc::clone(&apps)));

    let activated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let item_id = "chrome_status_icon_1";
    let app =
        put_up_item(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", item(item_id, activated.clone()));
    let address = format!("{}/StatusNotifierItem", app.unique_name().unwrap());
    let items = fixture.next_items();
    assert_eq!(items[0]["address"], json!(address));
    assert_eq!(items[0]["id"], "chrome_status_icon_1");
    assert_eq!(items[0]["title"], "chrome_status_icon_1 title");
    assert_eq!(items[0]["app"]["id"], "discord.desktop");
    let host = fixture.connect();
    assert_eq!(watcher_property::<Vec<String>>(&host, "RegisteredStatusNotifierItems"), std::slice::from_ref(&address));

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
    let app = eventually(WAIT, "the second tray to take over", || put_up_retrying(&fixture));
    assert_eq!(fixture.next_items()[0]["id"], "late");
    drop(app);
    second.stop();
}

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

    let tray = fixture.tray(None);
    tray.hide("steam", false).unwrap();
    assert!(event(&fixture).get("hidden").is_none());
    tray.stop();
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
        icon: None,
        attention: false,
        has_menu: false,
        is_menu: false,
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
    let other = Item { id: "other".into(), title: String::new(), activated: Arc::default() };
    zbus::block_on(app.object_server().at("/Other", other)).unwrap();
    zbus::block_on(register(&app, "/Nothing")).unwrap();
    wait_until(WAIT, "the item without an id to be dropped", || {
        watcher_property::<Vec<String>>(&app, "RegisteredStatusNotifierItems").is_empty()
    });
    let real = put_up(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", "steam");
    let items = loop {
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
    let _other = put_up(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", "steam");
    wait_until(WAIT, "the other app's item to be registered", || {
        watcher_property::<Vec<String>>(&app, "RegisteredStatusNotifierItems").len() > MAX_ITEMS_PER_APP
    });
}

fn entry(id: i32, label: &str, more: Vec<(&'static str, zbus::OwnedVariant)>, children: Vec<MenuNode>) -> MenuNode {
    let mut props = vec![
        ("label", OwnedVariant::from(zbus::Str::from(label.to_owned()))),
        ("enabled", true.into()),
        ("visible", true.into()),
    ];
    props.extend(more);
    MenuNode(id, props, children)
}

fn separator(id: i32) -> MenuNode {
    MenuNode(id, vec![("type", OwnedVariant::from(zbus::Str::from("separator")))], vec![])
}

fn discord_menu() -> MenuNode {
    let checkmark =
        || vec![("toggle-type", OwnedVariant::from(zbus::Str::from("checkmark"))), ("toggle-state", 1i32.into())];
    let submenu = vec![("children-display", OwnedVariant::from(zbus::Str::from("submenu")))];
    let hidden = vec![("visible", OwnedVariant::from(false))];
    MenuNode(
        0,
        vec![("children-display", OwnedVariant::from(zbus::Str::from("submenu")))],
        vec![
            entry(1, "Open Discord", vec![], vec![]),
            separator(2),
            entry(3, "Mute", checkmark(), vec![]),
            entry(4, "Voice && Video", submenu, vec![entry(5, "Input Device", vec![], vec![])]),
            entry(6, "Update Ready!", hidden, vec![]),
            separator(7),
            entry(8, "Quit Discord", vec![], vec![]),
        ],
    )
}

#[test]
fn an_item_s_picture_status_and_menu_are_read_for_the_gpui_ui() {
    let fixture = Fixture::new("look");
    std::fs::create_dir_all(fixture.dir.join("pictures")).unwrap();
    std::fs::write(fixture.dir.join("pictures/0-old.png"), b"left by an earlier novad").unwrap();
    let tray = fixture.tray(None);
    assert!(!fixture.dir.join("pictures/0-old.png").exists(), "emptied at start");

    let secondary = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let clicked = Arc::new(Mutex::new(Vec::new()));
    let shown = Arc::new(Mutex::new(Vec::new()));
    let app = fixture.connect();
    let menu = Menu { root: discord_menu(), revision: 4, clicked: Arc::clone(&clicked), shown: Arc::clone(&shown) };
    zbus::block_on(app.object_server().at("/MenuBar", menu)).unwrap();
    let item = LookItem {
        id: "chrome_status_icon_1".into(),
        status: "Active".into(),
        pixmap: (1, 1, vec![255, 0, 0, 255]),
        secondary: Arc::clone(&secondary),
    };
    serve_item(&app, None, "/StatusNotifierItem", "/StatusNotifierItem", item);
    let address = format!("{}/StatusNotifierItem", app.unique_name().unwrap());

    let items = fixture.next_items();
    let icon = PathBuf::from(items[0]["icon"].as_str().expect("a picture"));
    assert!(icon.starts_with(fixture.dir.join("pictures")) && icon.is_file(), "{icon:?}");
    assert_eq!(items[0]["has_menu"], true);
    assert!(items[0].get("attention").is_none());

    zbus::block_on(async {
        let iface = app.object_server().interface::<_, LookItem>("/StatusNotifierItem").await.unwrap();
        iface.get_mut().await.status = "NeedsAttention".into();
        LookItem::new_status(iface.signal_emitter(), "NeedsAttention").await.unwrap();
    });
    assert_eq!(fixture.next_items()[0]["attention"], true);

    let level = tray.menu_level(&address, 0).unwrap();
    assert_eq!(*shown.lock().unwrap(), [0], "told before it's read");
    assert_eq!(level.revision, 4);
    let labels: Vec<&str> = level.entries.iter().map(|entry| entry.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Open Discord", "", "Mute", "Voice && Video", "", "Quit Discord"],
        "the hidden entry is left out"
    );
    assert!(level.entries[1].separator);
    assert_eq!((level.entries[2].toggle, level.entries[2].checked), (nova_tray_api::Toggle::Checkmark, true));
    assert!(level.entries[3].submenu && !level.entries[0].submenu);
    let inner = tray.menu_level(&address, 4).unwrap();
    assert_eq!(inner.entries.iter().map(|entry| entry.id).collect::<Vec<_>>(), [5]);

    tray.run_menu_entry(&address, 3, 4).unwrap();
    assert_eq!(*clicked.lock().unwrap(), [3]);
    zbus::block_on(async {
        let iface = app.object_server().interface::<_, Menu>("/MenuBar").await.unwrap();
        iface.get_mut().await.revision = 5;
        Menu::layout_updated(iface.signal_emitter(), 5, 0).await.unwrap();
    });
    wait_until(WAIT, "the new revision to arrive", || tray.read_of(&address, |read| read.menu_revision) == Ok(Some(5)));
    let refused = tray.run_menu_entry(&address, 8, 4).unwrap_err();
    assert!(refused.contains("changed"), "{refused}");
    assert_eq!(*clicked.lock().unwrap(), [3], "nothing else was clicked");

    tray.secondary_activate(&address).unwrap();
    assert_eq!(secondary.load(Ordering::SeqCst), 1);

    drop(app);
    assert_eq!(fixture.next_items(), json!([]));
    assert!(!icon.exists(), "its picture goes with it");
    tray.stop();
}
