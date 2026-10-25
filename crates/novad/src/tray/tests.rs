//! The tray against a real bus: a private `dbus-daemon` per test, with apps
//! that put up items the way Electron (an object path on its own
//! connection) and libayatana (a bus name of its own) do.

use super::*;
use crate::hub::{ClientQueue, Outgoing};
use crate::launcher;
use nova_protocol::Topic;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// How long a test waits for something the bus should do at once.
const WAIT: Duration = Duration::from_secs(5);

struct Fixture {
    dir: PathBuf,
    bus: Child,
    address: String,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Receiver<Outgoing>,
    _ui_stream: UnixStream,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("novad-tray-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proc")).unwrap();
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
                dir.join("bus").display()
            ),
        )
        .unwrap();
        let mut bus = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon runs");
        let mut address = String::new();
        BufReader::new(bus.stdout.take().unwrap()).read_line(&mut address).unwrap();
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let (ui_stream, other_end) = UnixStream::pair().unwrap();
        let (queue, ui) = ClientQueue::new(&other_end).unwrap();
        let id = hub.register(queue);
        hub.subscribe(id, Topic::Tray);
        Self { dir, bus, address: address.trim().to_string(), hub, logger, ui, _ui_stream: ui_stream }
    }

    fn tray(&self, launcher: Option<Arc<Launcher>>) -> Arc<Tray> {
        let bus = Bus::Address(self.address.clone());
        Tray::start(bus, self.dir.join("proc"), launcher, Arc::clone(&self.hub), Arc::clone(&self.logger))
    }

    fn connect(&self) -> Connection {
        zbus::block_on(connection::Builder::address(self.address.as_str()).build()).unwrap()
    }

    /// The next tray event's items.
    fn next_items(&self) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.ui.recv_timeout(left).expect("a tray event") {
                Outgoing::Line(line) => {
                    let event: Value = serde_json::from_str(&line).unwrap();
                    if event["event"] == "tray" {
                        return event["data"]["items"].clone();
                    }
                }
                Outgoing::Flushed(_) => {}
            }
        }
    }

    /// Makes our process look, to the tray, like it runs in `unit`.
    fn run_in_unit(&self, unit: &str) {
        let process = self.dir.join("proc").join(std::process::id().to_string());
        std::fs::create_dir_all(&process).unwrap();
        std::fs::write(process.join("cgroup"), format!("0::/user.slice/app.slice/{unit}\n")).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// An app's tray item, with the two properties the tray reads.
struct Item {
    id: String,
    title: String,
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
}

/// Puts up an item at `path` on a new connection, owning `name` first if
/// given, and registers `service` with the watcher.
fn put_up(fixture: &Fixture, name: Option<&str>, path: &str, service: &str, id: &str) -> Connection {
    let connection = fixture.connect();
    zbus::block_on(async {
        let item = Item { id: id.into(), title: format!("{id} title") };
        connection.object_server().at(path, item).await.unwrap();
        if let Some(name) = name {
            connection.request_name(name).await.unwrap();
        }
        register(&connection, service).await.unwrap();
    });
    connection
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
    let paths = launcher::Paths {
        data_dirs: vec![fixture.dir.join("share")],
        state_file: fixture.dir.join("launcher.json"),
        runner: launcher::Runner::Direct,
    };
    let launcher = Launcher::start(paths, "", Arc::clone(&fixture.hub), Arc::clone(&fixture.logger));
    let deadline = Instant::now() + WAIT;
    while launcher.find_app(&["discord".into()]).is_none() {
        assert!(Instant::now() < deadline, "the launcher found discord.desktop");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Electron's own id doesn't name the app; the unit it runs in does.
    fixture.run_in_unit("app-discord-375306.scope");
    let tray = fixture.tray(Some(Arc::clone(&launcher)));

    let app = put_up(&fixture, None, "/StatusNotifierItem", "/StatusNotifierItem", "chrome_status_icon_1");
    let address = format!("{}/StatusNotifierItem", app.unique_name().unwrap());
    let items = fixture.next_items();
    assert_eq!(items[0]["address"], json!(address));
    assert_eq!(items[0]["id"], "chrome_status_icon_1");
    assert_eq!(items[0]["title"], "chrome_status_icon_1 title");
    assert_eq!(items[0]["app"]["id"], "discord.desktop");
    let host = fixture.connect();
    assert_eq!(watcher_property::<Vec<String>>(&host, "RegisteredStatusNotifierItems"), std::slice::from_ref(&address));

    drop(app);
    assert_eq!(fixture.next_items(), json!([]));
    assert!(watcher_property::<Vec<String>>(&host, "RegisteredStatusNotifierItems").is_empty());
    tray.stop();
    launcher.stop();
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
fn a_queued_tray_takes_over_when_the_first_stops() {
    let fixture = Fixture::new("queued");
    let first = fixture.tray(None);
    let second = fixture.tray(None);
    first.stop();
    let deadline = Instant::now() + WAIT;
    let app = loop {
        // Until the bus has given the name to the second tray.
        let app = put_up_retrying(&fixture);
        if let Some(app) = app {
            break app;
        }
        assert!(Instant::now() < deadline, "the second tray took over");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(fixture.next_items()[0]["id"], "late");
    drop(app);
    second.stop();
}

/// Puts up an item if the watcher it reaches is one that's running.
fn put_up_retrying(fixture: &Fixture) -> Option<Connection> {
    let connection = fixture.connect();
    zbus::block_on(async {
        let item = Item { id: "late".into(), title: String::new() };
        connection.object_server().at("/StatusNotifierItem", item).await.unwrap();
        register(&connection, "/StatusNotifierItem").await.ok()?;
        Some(())
    })?;
    Some(connection)
}
