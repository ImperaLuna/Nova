//! Component tests: novad driven through its real socket, with a real logger
//! and supervisor. The only stand-in is the UI process in the crash test, a
//! shell script, since Quickshell needs a compositor.

use super::*;
use nova_protocol::{FeatureStatus, LogEntry, UI_LOG_SOURCE};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Lines, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

struct TestDaemon {
    dir: PathBuf,
    socket: PathBuf,
    daemon: Arc<Daemon>,
}

impl TestDaemon {
    fn start(name: &str, ui: Option<UiSettings>) -> Self {
        let dir = std::env::temp_dir().join(format!("novad-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let socket = dir.join("novad.sock");
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let listener = server::bind(&socket).unwrap();
        let ui = ui.map(|settings| Supervisor::start(settings, Arc::clone(&logger)));
        let features = vec![FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] }];
        let (theme, _) = theme::resolve(&theme::default_dir(), &dir.join("no-config.toml"));
        let daemon =
            Arc::new(Daemon { started: Instant::now(), features, theme, hub, logger, ui, socket: socket.clone() });
        let serving = Arc::clone(&daemon);
        std::thread::spawn(move || server::serve(listener, serving));
        Self { dir, socket, daemon }
    }

    /// Connects and reads the hello.
    fn connect(&self) -> Client {
        let stream = UnixStream::connect(&self.socket).unwrap();
        let writer = stream.try_clone().unwrap();
        let mut client = Client { writer, lines: BufReader::new(stream).lines() };
        assert_eq!(client.next()["event"], "hello");
        client
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Client {
    writer: UnixStream,
    lines: Lines<BufReader<UnixStream>>,
}

impl Client {
    fn send(&mut self, message: &Value) {
        writeln!(self.writer, "{message}").unwrap();
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.writer, "{line}").unwrap();
    }

    fn next(&mut self) -> Value {
        serde_json::from_str(&self.lines.next().unwrap().unwrap()).unwrap()
    }

    fn request(&mut self, id: u64, cmd: &str, args: Option<Value>) -> Value {
        let mut message = json!({"v": 2, "id": id, "cmd": cmd});
        if let Some(args) = args {
            message["args"] = args;
        }
        self.send(&message);
        let reply = self.next();
        assert_eq!(reply["id"], id, "expected the reply first, got {reply}");
        reply
    }
}

#[test]
fn only_hello_arrives_until_subscribed() {
    let novad = TestDaemon::start("hello", None);
    let mut client = novad.connect();
    assert_eq!(client.request(1, "ping", None)["result"], "pong");
}

#[test]
fn subscribing_to_features_sends_their_state() {
    let novad = TestDaemon::start("features", None);
    let mut client = novad.connect();
    let reply = client.request(1, "subscribe", Some(json!({"topics": ["features"]})));
    assert_eq!(reply["result"], Value::Null);
    let event = client.next();
    assert_eq!(event["event"], "features");
    assert_eq!(event["data"]["features"][0], json!({"id": "core", "status": "ready"}));
}

#[test]
fn subscribing_to_the_theme_sends_its_colors() {
    let novad = TestDaemon::start("theme", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    let event = client.next();
    assert_eq!(event["event"], "theme");
    assert_eq!(event["data"]["id"], theme::DEFAULT_THEME_ID);
    assert_eq!(event["data"]["mode"], "dark");
    assert_eq!(event["data"]["colors"]["primary"], "#FD7014");
}

#[test]
fn requests_without_id_get_no_reply() {
    let novad = TestDaemon::start("no-id", None);
    let mut client = novad.connect();
    client.send(&json!({"v": 2, "cmd": "ping"}));
    assert_eq!(client.request(2, "ping", None)["result"], "pong");
}

#[test]
fn bad_requests_get_an_error_when_they_have_an_id() {
    let novad = TestDaemon::start("bad", None);
    let mut client = novad.connect();
    client.send_raw("not json");
    client.send(&json!({"v": 2, "id": 4, "cmd": "reboot"}));
    let reply = client.next();
    assert_eq!(reply["id"], 4);
    assert!(reply["error"].as_str().unwrap().starts_with("bad request"));
}

#[test]
fn unknown_fields_and_other_versions_are_refused() {
    let novad = TestDaemon::start("strict", None);
    let mut client = novad.connect();
    client.send(&json!({"v": 2, "id": 1, "cmd": "ping", "extra": true}));
    assert!(client.next()["error"].as_str().unwrap().contains("unknown field"));
    client.send(&json!({"v": 1, "id": 2, "cmd": "ping"}));
    assert!(client.next()["error"].as_str().unwrap().contains("version 1"));
    assert_eq!(client.request(3, "ping", None)["result"], "pong");
}

#[test]
fn oversized_requests_close_the_connection() {
    let novad = TestDaemon::start("oversized", None);
    let mut client = novad.connect();
    let huge = "x".repeat(usize::try_from(server::MAX_REQUEST_BYTES).unwrap() + 10);
    client.send_raw(&huge);
    assert!(client.lines.next().is_none(), "novad should hang up");
}

#[test]
fn a_client_that_stops_reading_is_disconnected() {
    let novad = TestDaemon::start("slow-client", None);
    let mut stalled = novad.connect();
    stalled.request(1, "subscribe", Some(json!({"topics": ["logs"]})));
    // Far more entries than the socket buffer and the queue hold together.
    for entry in 0..crate::hub::MAX_QUEUED_MESSAGES * 8 {
        novad.daemon.logger.info("test", &format!("entry {entry}"));
    }
    let delivered = stalled.lines.by_ref().map_while(Result::ok).count();
    assert!(delivered < crate::hub::MAX_QUEUED_MESSAGES * 8, "the stalled client was never cut off");
    let mut other = novad.connect();
    assert_eq!(other.request(1, "ping", None)["result"], "pong");
}

#[test]
fn a_second_novad_on_the_same_socket_is_refused() {
    let novad = TestDaemon::start("twice", None);
    let error = server::bind(&novad.socket).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
}

#[test]
fn log_subscribers_get_a_backlog_then_live_entries() {
    let novad = TestDaemon::start("logs", None);
    novad.daemon.logger.info("test", "before");
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["logs"]})));

    let backlog = client.next();
    assert_eq!(backlog["data"]["backlog"], true);
    let messages: Vec<&str> =
        backlog["data"]["entries"].as_array().unwrap().iter().map(|e| e["message"].as_str().unwrap()).collect();
    assert!(messages.contains(&"before"), "{messages:?}");

    novad.daemon.logger.warn("test", "after");
    let live = client.next();
    assert_eq!(live["data"].get("backlog"), None);
    assert_eq!(live["data"]["entries"][0]["message"], "after");
    assert_eq!(live["data"]["entries"][0]["level"], "warn");
}

#[test]
fn status_reports_the_daemon_ui_and_features() {
    let novad = TestDaemon::start("status", None);
    let mut client = novad.connect();
    let status: Status = serde_json::from_value(client.request(1, "status", None)["result"].clone()).unwrap();
    assert_eq!(status.pid, std::process::id());
    assert_eq!(status.ui.state, UiState::Disabled);
    assert_eq!(status.features[0].id, "core");
    assert!(status.pss_kib.is_some());
}

#[test]
fn a_crashing_ui_is_logged_snapshotted_and_restarted() {
    let scripts = std::env::temp_dir().join(format!("novad-test-ui-{}", std::process::id()));
    std::fs::create_dir_all(&scripts).unwrap();
    let fake_ui = scripts.join("quickshell");
    std::fs::write(&fake_ui, "#!/bin/sh\necho '  WARN qml: about to fail'\nexit 3\n").unwrap();
    std::fs::set_permissions(&fake_ui, std::fs::Permissions::from_mode(0o755)).unwrap();

    let settings = UiSettings {
        quickshell: fake_ui,
        ui_dir: PathBuf::from("ui"),
        socket: PathBuf::from("unused.sock"),
        path_var: std::env::var("PATH").unwrap_or_default(),
        wayland_socket: None,
    };
    let novad = TestDaemon::start("crash", Some(settings));
    let mut client = novad.connect();
    let status = wait_for_status(&mut client, |status| status.ui.restarts >= 1);

    let crash_log = PathBuf::from(&status.ui.last_crash.unwrap().log);
    let crash_entries = read_entries(&crash_log);
    let ui_line = crash_entries.iter().find(|entry| entry.source == UI_LOG_SOURCE).unwrap();
    assert_eq!((ui_line.level, ui_line.message.as_str()), (LogLevel::Warn, "qml: about to fail"));
    assert!(crash_entries.iter().any(|entry| entry.message.contains("exit code 3")));
    let full_log = read_entries(&nova_paths::log_file(&novad.dir.join("logs")));
    assert!(full_log.iter().any(|entry| entry.message.contains("failed to load")));
    std::fs::remove_dir_all(scripts).unwrap();
}

fn wait_for_status(client: &mut Client, done: impl Fn(&Status) -> bool) -> Status {
    for attempt in 0..100 {
        let status: Status = serde_json::from_value(client.request(attempt, "status", None)["result"].clone()).unwrap();
        if done(&status) {
            return status;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("status never got there");
}

fn read_entries(path: &Path) -> Vec<LogEntry> {
    std::fs::read_to_string(path).unwrap().lines().filter_map(LogEntry::parse_line).collect()
}

#[test]
fn manifest_daemon_names_match_cargo_features() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo: toml::Table = std::fs::read_to_string(root.join("Cargo.toml")).unwrap().parse().unwrap();
    let mut cargo_features: Vec<String> = cargo
        .get("features")
        .and_then(|f| f.as_table())
        .map(|t| t.keys().filter(|k| *k != "default").cloned().collect())
        .unwrap_or_default();
    let mut declared: Vec<String> =
        nova_features::load_all(&root.join("../../features")).unwrap().into_iter().filter_map(|m| m.daemon).collect();
    cargo_features.sort();
    declared.sort();
    assert_eq!(cargo_features, declared, "novad [features] and manifest `daemon` fields differ");
}
