//! Component tests: novad driven through its real socket, with a real logger,
//! supervisor, settings files and feature manifests. The only stand-in is the
//! UI process in the crash test, a shell script, since Quickshell needs a
//! compositor.

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
    watching: Option<inotify::Stopper>,
}

/// `features.toml` with features off. The features with a watcher of their own, off unless a test is about
/// them: each watcher is an inotify instance, and the tests running at once
/// would pass the usual limit of 128.
const WATCHERS_OFF: &str = "disabled = [\"ai-usage\", \"launcher\"]\n";
const LAUNCHER_OFF: &str = "disabled = [\"launcher\"]\n";
const AI_USAGE_OFF: &str = "disabled = [\"ai-usage\"]\n";

impl TestDaemon {
    /// With the `ai-usage` and `launcher` features off.
    fn start(name: &str, ui: Option<UiSettings>) -> Self {
        Self::open(name, ui, |dir| std::fs::write(dir.join("config/features.toml"), WATCHERS_OFF).unwrap())
    }

    /// With the `ai-usage` feature on, its files under `<dir>/ai-usage`, and
    /// the `launcher` off.
    fn with_ai_usage(name: &str, prepare: impl FnOnce(&Path)) -> Self {
        Self::open(name, None, |dir| {
            std::fs::write(dir.join("config/features.toml"), LAUNCHER_OFF).unwrap();
            prepare(&dir.join("ai-usage"));
        })
    }

    /// With the `launcher` feature on, its apps under
    /// `<dir>/launcher/share`, and `ai-usage` off.
    fn with_launcher(name: &str) -> Self {
        Self::open(name, None, |dir| std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap())
    }

    /// Like [`TestDaemon::with_launcher`], following the compositor `connect`
    /// sets up in the test's folder.
    fn with_launcher_following(name: &str, connect: impl FnOnce(&Path) -> Box<dyn compositor::Backend>) -> Self {
        Self::open_following(
            name,
            None,
            |dir| std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap(),
            |dir| Some(connect(dir)),
        )
    }

    /// Following the compositor `connect` sets up in the test's folder, with
    /// the `ai-usage` and `launcher` features off.
    pub fn with_compositor(name: &str, connect: impl FnOnce(&Path) -> Box<dyn compositor::Backend>) -> Self {
        Self::open_following(
            name,
            None,
            |dir| std::fs::write(dir.join("config/features.toml"), WATCHERS_OFF).unwrap(),
            |dir| Some(connect(dir)),
        )
    }

    /// Settings files go in `<dir>/config`, which `prepare` may fill or remove.
    fn open(name: &str, ui: Option<UiSettings>, prepare: impl FnOnce(&Path)) -> Self {
        Self::open_following(name, ui, prepare, |_| None)
    }

    /// Like [`TestDaemon::open`], following the compositor `compositor` sets up.
    fn open_following(
        name: &str,
        ui: Option<UiSettings>,
        prepare: impl FnOnce(&Path),
        compositor: impl FnOnce(&Path) -> Option<Box<dyn compositor::Backend>>,
    ) -> Self {
        let dir = std::env::temp_dir().join(format!("novad-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::create_dir_all(dir.join("ai-usage")).unwrap();
        std::fs::create_dir_all(dir.join("launcher/share/applications")).unwrap();
        prepare(&dir);
        let socket = dir.join("novad.sock");
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let listener = server::bind(&socket).unwrap();
        let ui = ui.map(|settings| Supervisor::start(settings, Arc::clone(&logger)));
        let sources = Sources {
            config: dir.join("config/config.toml"),
            selection: dir.join("config/features.toml"),
            themes_dir: theme::default_dir(),
            manifests: nova_features::load_all(&repo_features()).unwrap(),
            path_var: std::env::var("PATH").unwrap_or_default(),
            ai_usage: ai_usage_paths(&dir.join("ai-usage")),
            resources: resources::Sources::system(""),
            launcher: launcher_paths(&dir),
        };
        let settings = Settings::load(&sources, &hub, &logger);
        let compositor = compositor(&dir)
            .map(|backend| compositor::Compositor::start(backend, Arc::clone(&hub), Arc::clone(&logger)));
        let session_lock = Arc::new(session_lock::SessionLock::open(&dir, &logger));
        let screenshots = Arc::new(screenshot::Screenshots::new(
            &dir,
            sources.path_var.clone(),
            Arc::clone(&hub),
            Arc::clone(&logger),
        ));
        let recordings =
            Arc::new(recording::Recordings::new(sources.path_var.clone(), Arc::clone(&hub), Arc::clone(&logger)));
        let daemon = Arc::new(Daemon {
            started: Instant::now(),
            hub,
            logger,
            ui,
            socket: socket.clone(),
            sources,
            settings: Mutex::new(settings),
            compositor,
            session_lock,
            screenshots,
            recordings,
        });
        daemon.recordings.allow(daemon.feature_ready(recording::FEATURE_ID));
        let watching = daemon.watch_settings();
        let serving = Arc::clone(&daemon);
        std::thread::spawn(move || server::serve(listener, serving));
        Self { dir, socket, daemon, watching }
    }

    fn config_file(&self) -> PathBuf {
        self.daemon.sources.config.clone()
    }

    fn features_file(&self) -> PathBuf {
        self.daemon.sources.selection.clone()
    }

    /// Connects and reads the hello.
    fn connect(&self) -> Client {
        let stream = UnixStream::connect(&self.socket).unwrap();
        let writer = stream.try_clone().unwrap();
        let mut client = Client { writer, lines: BufReader::new(stream).lines(), early: Default::default() };
        assert_eq!(client.next()["event"], "hello");
        client
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        // Their file watchers go with them: a user gets only so many
        // (fs.inotify.max_user_instances, 128 by default), and tests run in
        // one process.
        let settings = lock(&self.daemon.settings);
        if let Some(launcher) = &settings.launcher {
            launcher.stop();
        }
        if let Some(usage) = &settings.ai_usage {
            usage.stop();
        }
        drop(settings);
        if let Some(watching) = &self.watching {
            watching.stop();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Client {
    writer: UnixStream,
    lines: Lines<BufReader<UnixStream>>,
    /// Events that arrived while `request` waited for its reply.
    early: std::collections::VecDeque<Value>,
}

impl Client {
    /// The data of the next `name` event, skipping anything else; fails
    /// after a while.
    fn next_event(&mut self, name: &str) -> Value {
        self.writer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        loop {
            let message = match self.early.pop_front() {
                Some(event) => event,
                None => {
                    let line = self.lines.next().expect("connection closed").expect("no event in time");
                    serde_json::from_str(&line).unwrap()
                }
            };
            if message["event"] == name {
                return message["data"].clone();
            }
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.writer, "{message}").unwrap();
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.writer, "{line}").unwrap();
    }

    fn next(&mut self) -> Value {
        if let Some(event) = self.early.pop_front() {
            return event;
        }
        serde_json::from_str(&self.lines.next().unwrap().unwrap()).unwrap()
    }

    fn request(&mut self, id: u64, cmd: &str, args: Option<Value>) -> Value {
        let mut message = json!({"v": 2, "id": id, "cmd": cmd});
        if let Some(args) = args {
            message["args"] = args;
        }
        self.send(&message);
        // Events published meanwhile (ai_usage's worker) may come first;
        // they're kept for `next`.
        loop {
            let message: Value = serde_json::from_str(&self.lines.next().unwrap().unwrap()).unwrap();
            if message.get("event").is_none() {
                assert_eq!(message["id"], id, "expected the reply to {id}, got {message}");
                return message;
            }
            self.early.push_back(message);
        }
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
    assert!(event["data"]["features"].as_array().unwrap().contains(&json!({"id": "core", "status": "ready"})));
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
fn a_client_that_stalls_with_its_socket_full_is_disconnected() {
    let novad = TestDaemon::start("stalled-client", None);
    let mut stalled = novad.connect();
    stalled.request(1, "subscribe", Some(json!({"topics": ["logs"]})));
    // Fills the socket buffer many times over but not the queue, so only the
    // write deadline can cut the client off.
    let entries = crate::hub::MAX_QUEUED_MESSAGES / 2;
    let padding = "x".repeat(1024);
    for entry in 0..entries {
        novad.daemon.logger.info("test", &format!("entry {entry} {padding}"));
    }
    std::thread::sleep(server::WRITE_TIMEOUT * 3);
    // Without the deadline everything arrives and the connection stays open.
    stalled.writer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let delivered = stalled.lines.by_ref().map_while(Result::ok).count();
    assert!(delivered < entries, "the stalled client was never cut off");
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
    assert!(status.features.iter().any(|feature| feature.id == "core" && feature.status == FeatureStatus::Ready));
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

#[test]
fn a_quickshell_outside_the_tested_range_is_warned_about() {
    let scripts = std::env::temp_dir().join(format!("novad-test-old-quickshell-{}", std::process::id()));
    std::fs::create_dir_all(&scripts).unwrap();
    let fake_ui = scripts.join("quickshell");
    let script =
        "#!/bin/sh\ncase \"$1\" in --version) echo 'Quickshell 0.1.0 (revision test)' ;; *) exec sleep 60 ;; esac\n";
    std::fs::write(&fake_ui, script).unwrap();
    std::fs::set_permissions(&fake_ui, std::fs::Permissions::from_mode(0o755)).unwrap();

    let settings = UiSettings {
        quickshell: fake_ui,
        ui_dir: PathBuf::from("ui"),
        socket: PathBuf::from("unused.sock"),
        path_var: std::env::var("PATH").unwrap_or_default(),
        wayland_socket: None,
    };
    let novad = TestDaemon::start("quickshell-range", Some(settings));
    let status = wait_for_status(&mut novad.connect(), |status| status.ui.pid.is_some());
    assert_eq!(status.quickshell.as_deref(), Some("0.1.0"));
    let log = read_entries(&nova_paths::log_file(&novad.dir.join("logs")));
    let warning = log.iter().find(|entry| entry.level == LogLevel::Warn && entry.message.contains("0.1.0"));
    assert!(warning.is_some_and(|entry| entry.message.contains("older than Nova is tested with")), "{log:?}");
    novad.daemon.ui.as_ref().unwrap().stop();
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

/// Apps come from `<dir>/launcher/share` only, and start forked twice,
/// never as services of the desktop's systemd.
fn launcher_paths(dir: &Path) -> launcher::Paths {
    launcher::Paths {
        data_dirs: vec![dir.join("launcher/share")],
        state_file: dir.join("launcher/launcher.json"),
        runner: launcher::Runner::Direct,
    }
}

fn repo_features() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features")
}

/// The usage endpoints are `claude_usage_url` and `codex_usage_url`; nothing
/// listens there unless a test starts a fake one.
fn ai_usage_paths(root: &Path) -> ai_usage::Paths {
    ai_usage::Paths {
        codex_home: Some(root.join("codex")),
        state_file: root.join("state/ai-usage.json"),
        claude_credentials: Some(root.join("claude/.credentials.json")),
        claude_usage_url: std::fs::read_to_string(root.join("claude/url")).unwrap_or("http://127.0.0.1:9/".into()),
        codex_usage_url: std::fs::read_to_string(root.join("codex-url")).unwrap_or("http://127.0.0.1:9/".into()),
    }
}

/// `config.toml` with Claude's usage endpoint turned on.
const CLAUDE_ENDPOINT_ON: &str = "[ai-usage]\nclaude-usage-endpoint = true\n";
/// `config.toml` with Codex's usage endpoint turned on.
const CODEX_ENDPOINT_ON: &str = "[ai-usage]\ncodex-usage-endpoint = true\n";

/// Answers every request with `body`, keeping the request heads it got.
fn fake_endpoint(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    fake_endpoint_gated(body, None)
}

/// Like [`fake_endpoint`], answering each request only once `gate` lets it.
fn fake_endpoint_gated(
    body: &'static str,
    gate: Option<std::sync::mpsc::Receiver<()>>,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut head = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while reader.read_line(&mut head).unwrap() > 2 {}
            seen.lock().unwrap().push(head);
            if let Some(gate) = &gate {
                gate.recv().unwrap();
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (url, requests)
}

/// Claude Code's credentials file, signed in to a Max plan, and the usage
/// endpoint at `url` (turned on in `config.toml` separately).
fn claude_sign_in(root: &Path, url: &str) {
    let dir = root.join("claude");
    std::fs::create_dir_all(&dir).unwrap();
    let expires = jiff::Timestamp::now().as_millisecond() + 3_600_000;
    let credentials =
        format!(r#"{{"claudeAiOauth":{{"accessToken":"tok","expiresAt":{expires},"subscriptionType":"max"}}}}"#);
    let path = dir.join(".credentials.json");
    std::fs::write(&path, credentials).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    std::fs::write(dir.join("url"), url).unwrap();
}

/// Codex's sign-in with a ChatGPT account, and the usage endpoint at `url`
/// (turned on in `config.toml` separately).
fn codex_sign_in(root: &Path, url: &str) {
    let home = root.join("codex");
    std::fs::create_dir_all(&home).unwrap();
    let path = home.join("auth.json");
    std::fs::write(&path, r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"tok","account_id":"work"}}"#).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    std::fs::write(root.join("codex-url"), url).unwrap();
}

/// OpenAI's answer for a Plus plan with only a weekly window.
const CODEX_ANSWER: &str = r#"{"plan_type":"plus","rate_limit":{"allowed":true,"limit_reached":false,"primary_window":{"used_percent":6,"limit_window_seconds":604800,"reset_after_seconds":400000,"reset_at":4102444800},"secondary_window":null},"credits":null}"#;

/// A Codex `token_count` log line in the shape Codex writes, with limits
/// that reset an hour and three days from now.
fn codex_line(session: u8, weekly: u8) -> String {
    let now = jiff::Timestamp::now().as_second();
    format!(
        r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"token_count","info":null,"rate_limits":{{"primary":{{"used_percent":{session}.0,"window_minutes":300,"resets_at":{}}},"secondary":{{"used_percent":{weekly}.0,"window_minutes":10080,"resets_at":{}}},"plan_type":"plus"}}}}}}"#,
        jiff::Timestamp::now(),
        now + 3600,
        now + 3 * 86400,
    )
}

fn codex_day(root: &Path) -> PathBuf {
    let day = root.join("codex/sessions/2026/10/23");
    std::fs::create_dir_all(&day).unwrap();
    day
}

impl Client {
    /// The next `ai_usage` event's data, skipping anything else.
    fn next_usage(&mut self) -> Value {
        self.next_event("ai_usage")
    }

    fn subscribe_usage(&mut self) -> Value {
        self.request(1, "subscribe", Some(json!({"topics": ["ai_usage"]})));
        let mut usage = self.next_usage();
        while usage["providers"].as_array().unwrap().iter().any(|p| p["state"] == "loading") {
            usage = self.next_usage();
        }
        usage
    }
}

fn provider<'a>(usage: &'a Value, id: &str) -> &'a Value {
    usage["providers"].as_array().unwrap().iter().find(|p| p["provider"] == id).unwrap()
}

#[test]
fn ai_usage_starts_with_setup_hints() {
    let novad = TestDaemon::with_ai_usage("ai-usage-empty", |_| {});
    let usage = novad.connect().subscribe_usage();
    assert_eq!(usage["shown"], "claude");
    let claude = provider(&usage, "claude");
    assert_eq!(claude["state"], "unavailable");
    assert!(claude["hint"].as_str().unwrap().contains("claude-usage-endpoint = true"), "{claude}");
    assert_eq!(provider(&usage, "codex")["state"], "unavailable");
}

#[test]
fn claude_limits_come_from_anthropic_with_claude_codes_sign_in() {
    let (url, requests) = fake_endpoint(
        r#"{"five_hour":{"utilization":8.4,"resets_at":"2100-01-01T05:00:00.451561+00:00"},"seven_day":{"utilization":17.0,"resets_at":"2100-01-07T00:00:00+00:00"},"seven_day_opus":null}"#,
    );
    let novad = TestDaemon::with_ai_usage("ai-usage-claude-api", |root| {
        claude_sign_in(root, &url);
        std::fs::write(root.join("../config/config.toml"), CLAUDE_ENDPOINT_ON).unwrap();
    });
    let mut client = novad.connect();
    let claude = provider(&client.subscribe_usage(), "claude").clone();
    assert_eq!((claude["state"].as_str(), claude["plan"].as_str()), (Some("ready"), Some("Max")));
    assert_eq!(
        claude["limits"],
        json!([
            {"window": "session", "percent": 8, "resets_at": "2100-01-01T05:00:00.451561Z"},
            {"window": "weekly", "percent": 17, "resets_at": "2100-01-07T00:00:00Z"}
        ])
    );
    let head = requests.lock().unwrap()[0].to_lowercase();
    assert!(head.contains("authorization: bearer tok\r\n") && head.contains("anthropic-beta: oauth-2025-04-20\r\n"));
    assert!(head.contains("user-agent: nova/"), "{head}");

    // Opening the panel right after the first answer doesn't ask again.
    client.request(2, "refresh_ai_usage", None);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn a_status_line_report_beats_an_older_endpoint_answer() {
    let (open_gate, gate) = std::sync::mpsc::channel();
    let (url, requests) = fake_endpoint_gated(r#"{"five_hour":{"utilization":8.4,"resets_at":null}}"#, Some(gate));
    let novad = TestDaemon::with_ai_usage("ai-usage-claude-order", |root| {
        claude_sign_in(root, &url);
        std::fs::write(root.join("../config/config.toml"), CLAUDE_ENDPOINT_ON).unwrap();
    });
    let mut client = novad.connect();
    while requests.lock().unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(10));
    }
    // While the request is out, the status line reports newer numbers.
    let reply =
        client.request(2, "report_claude_usage", Some(json!({"limits": [{"window": "session", "percent": 50}]})));
    assert_eq!(reply["result"], Value::Null);
    open_gate.send(()).unwrap();
    // The answer is dropped: a later subscriber still sees the report.
    std::thread::sleep(Duration::from_millis(300));
    let usage = novad.connect().subscribe_usage();
    assert_eq!(provider(&usage, "claude")["limits"][0]["percent"], 50, "{usage}");
}

#[test]
fn busy_codex_sessions_are_still_read() {
    let mut log = PathBuf::new();
    let novad = TestDaemon::with_ai_usage("ai-usage-codex-busy", |root| {
        log = codex_day(root).join("rollout-a.jsonl");
        std::fs::write(&log, codex_line(1, 1) + "\n").unwrap();
    });
    let mut client = novad.connect();
    assert_eq!(provider(&client.subscribe_usage(), "codex")["limits"][0]["percent"], 1);
    // Lines faster than the settle time, for longer than the longest delay.
    let writing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let still_writing = Arc::clone(&writing);
    let writer = std::thread::spawn(move || {
        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        for _ in 0..80 {
            writeln!(file, "{}", codex_line(2, 2)).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        }
        still_writing.store(false, std::sync::atomic::Ordering::SeqCst);
    });
    while provider(&client.next_usage(), "codex")["limits"][0]["percent"] != 2 {}
    assert!(writing.load(std::sync::atomic::Ordering::SeqCst), "read only once the writes stopped");
    writer.join().unwrap();
}

#[test]
fn claudes_endpoint_is_never_asked_unless_turned_on() {
    let (url, requests) = fake_endpoint(r#"{"five_hour":{"utilization":8.4,"resets_at":null}}"#);
    let novad = TestDaemon::with_ai_usage("ai-usage-claude-off", |root| claude_sign_in(root, &url));
    let mut client = novad.connect();
    let claude = provider(&client.subscribe_usage(), "claude").clone();
    assert_eq!(claude["state"], "unavailable");
    assert!(claude["hint"].as_str().unwrap().contains("doesn't document"), "{claude}");
    client.request(2, "refresh_ai_usage", None);
    std::thread::sleep(Duration::from_millis(300));
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn codex_limits_come_from_its_newest_log_and_follow_new_lines() {
    let mut log = PathBuf::new();
    let novad = TestDaemon::with_ai_usage("ai-usage-codex", |root| {
        let day = codex_day(root);
        let old = day.join("rollout-old.jsonl");
        std::fs::write(&old, codex_line(1, 2) + "\n").unwrap();
        // Written microseconds before the new one, it would share its time.
        let past = std::time::SystemTime::now() - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(past).unwrap();
        log = day.join("rollout-new.jsonl");
        std::fs::write(
            &log,
            r#"{"timestamp":"2026-10-23T06:00:00Z","type":"session_meta","payload":{}}"#.to_owned()
                + "\n"
                + &codex_line(12, 84)
                + "\n",
        )
        .unwrap();
    });
    let mut client = novad.connect();
    let codex = provider(&client.subscribe_usage(), "codex").clone();
    assert_eq!((codex["state"].as_str(), codex["plan"].as_str()), (Some("ready"), Some("Plus")));
    assert_eq!(codex["limits"][0]["window"], "session");
    assert_eq!(codex["limits"][0]["percent"], 12);
    assert_eq!(codex["limits"][1]["percent"], 84);

    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    writeln!(file, "{}", codex_line(13, 85)).unwrap();
    // An event for the first line may still be on its way; the new line's
    // follows it (next_usage fails if it never comes).
    while provider(&client.next_usage(), "codex")["limits"][0]["percent"] != 13 {}
}

/// The logs only change when Codex runs a turn; the endpoint is current.
#[test]
fn codex_limits_come_from_openai_with_codexs_sign_in() {
    let (url, requests) = fake_endpoint(CODEX_ANSWER);
    let mut log = PathBuf::new();
    let novad = TestDaemon::with_ai_usage("ai-usage-codex-api", |root| {
        codex_sign_in(root, &url);
        std::fs::write(root.join("../config/config.toml"), CODEX_ENDPOINT_ON).unwrap();
        // An older turn's numbers.
        log = codex_day(root).join("rollout-a.jsonl");
        std::fs::write(&log, codex_line(1, 8) + "\n").unwrap();
    });
    let mut client = novad.connect();
    let mut codex = provider(&client.subscribe_usage(), "codex").clone();
    while codex["limits"][0]["percent"] != 6 {
        codex = provider(&client.next_usage(), "codex").clone();
    }
    assert_eq!(codex["plan"], "Plus");
    assert_eq!(codex["limits"], json!([{"window": "weekly", "percent": 6, "resets_at": "2100-01-01T00:00:00Z"}]));
    let head = requests.lock().unwrap()[0].to_lowercase();
    assert!(head.contains("authorization: bearer tok\r\n") && head.contains("chatgpt-account-id: work\r\n"), "{head}");
    assert!(head.contains("user-agent: nova/"), "{head}");

    // A turn after the answer logs newer numbers, which win.
    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    writeln!(file, "{}", codex_line(2, 9)).unwrap();
    while provider(&client.next_usage(), "codex")["limits"][0]["percent"] != 2 {}
}

/// Codex's first turn creates its sessions folder before it logs limits;
/// the endpoint's numbers stay meanwhile.
#[test]
fn codex_endpoint_numbers_outlast_a_rescan_without_limits() {
    let (url, _requests) = fake_endpoint(CODEX_ANSWER);
    let mut root_dir = PathBuf::new();
    let novad = TestDaemon::with_ai_usage("ai-usage-codex-api-rescan", |root| {
        codex_sign_in(root, &url);
        std::fs::write(root.join("../config/config.toml"), CODEX_ENDPOINT_ON).unwrap();
        root_dir = root.to_owned();
    });
    let mut client = novad.connect();
    let mut codex = provider(&client.subscribe_usage(), "codex").clone();
    while codex["state"] != "ready" {
        codex = provider(&client.next_usage(), "codex").clone();
    }
    std::fs::write(codex_day(&root_dir).join("rollout-a.jsonl"), "").unwrap();
    // Give the watcher time to rescan.
    std::thread::sleep(Duration::from_millis(300));
    let codex = provider(&novad.connect().subscribe_usage(), "codex").clone();
    assert_eq!((codex["state"].as_str(), &codex["limits"][0]["percent"]), (Some("ready"), &json!(6)), "{codex}");
}

#[test]
fn codexs_endpoint_is_never_asked_unless_turned_on() {
    let (url, requests) = fake_endpoint(CODEX_ANSWER);
    let novad = TestDaemon::with_ai_usage("ai-usage-codex-off", |root| {
        codex_sign_in(root, &url);
        std::fs::write(codex_day(root).join("rollout-a.jsonl"), codex_line(1, 8) + "\n").unwrap();
    });
    let mut client = novad.connect();
    assert_eq!(provider(&client.subscribe_usage(), "codex")["limits"][0]["percent"], 1);
    client.request(2, "refresh_ai_usage", None);
    std::thread::sleep(Duration::from_millis(300));
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn codex_sessions_created_later_are_picked_up() {
    codex_appears_later("ai-usage-codex-later", true);
}

#[test]
fn codex_installed_after_novad_started_is_picked_up() {
    codex_appears_later("ai-usage-codex-installed-later", false);
}

/// Codex's sessions (and its home too, without `home_exists`) appear after
/// novad started.
fn codex_appears_later(name: &str, home_exists: bool) {
    let mut root_dir = PathBuf::new();
    let novad = TestDaemon::with_ai_usage(name, |root| {
        if home_exists {
            std::fs::create_dir_all(root.join("codex")).unwrap();
        }
        root_dir = root.to_owned();
    });
    let mut client = novad.connect();
    assert_eq!(provider(&client.subscribe_usage(), "codex")["state"], "unavailable");
    let day = codex_day(&root_dir);
    // Give the watcher a moment to watch the new folders before writing.
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(day.join("rollout-a.jsonl"), codex_line(7, 8) + "\n").unwrap();
    loop {
        let usage = client.next_usage();
        if provider(&usage, "codex")["state"] == "ready" {
            assert_eq!(provider(&usage, "codex")["limits"][0]["percent"], 7);
            break;
        }
    }
}

#[test]
fn a_replaced_codex_home_is_followed() {
    let mut root_dir = PathBuf::new();
    let novad = TestDaemon::with_ai_usage("ai-usage-codex-replaced", |root| {
        std::fs::write(codex_day(root).join("rollout-a.jsonl"), codex_line(7, 8) + "\n").unwrap();
        root_dir = root.to_owned();
    });
    let mut client = novad.connect();
    assert_eq!(provider(&client.subscribe_usage(), "codex")["limits"][0]["percent"], 7);
    // Restored from a backup: a new home moved into place of the old one.
    let restored = root_dir.join("restored");
    let day = restored.join("sessions/2026/10/23");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-b.jsonl"), codex_line(9, 10) + "\n").unwrap();
    std::fs::rename(root_dir.join("codex"), root_dir.join("codex.old")).unwrap();
    std::fs::rename(&restored, root_dir.join("codex")).unwrap();
    while provider(&client.next_usage(), "codex")["limits"][0]["percent"] != 9 {}
    // And it's watched: new lines in the new home arrive too.
    std::thread::sleep(Duration::from_millis(200));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(root_dir.join("codex/sessions/2026/10/23/rollout-b.jsonl"))
        .unwrap();
    writeln!(file, "{}", codex_line(11, 12)).unwrap();
    while provider(&client.next_usage(), "codex")["limits"][0]["percent"] != 11 {}
}

#[test]
fn claude_reports_and_the_bar_choice_survive_a_restart() {
    let novad = TestDaemon::with_ai_usage("ai-usage-claude", |_| {});
    let mut client = novad.connect();
    client.subscribe_usage();
    let resets_at = (jiff::Timestamp::now() + jiff::SignedDuration::from_hours(2)).to_string();
    let limits = json!([
        {"window": "session", "percent": 42, "resets_at": resets_at},
        {"window": "weekly", "percent": 61}
    ]);
    assert_eq!(client.request(2, "report_claude_usage", Some(json!({"limits": limits})))["result"], Value::Null);
    let claude = provider(&client.next_usage(), "claude").clone();
    assert_eq!(claude["state"], "ready");
    assert_eq!(claude["limits"], limits);

    client.request(3, "show_ai_usage", Some(json!({"provider": "codex"})));
    assert_eq!(client.next_usage()["shown"], "codex");

    // A second novad on the same files starts where the first left off.
    let again = ai_usage::Usage::start(
        ai_usage_paths(&novad.dir.join("ai-usage")),
        &config::AiUsageConfig::default(),
        Arc::new(Hub::default()),
        Arc::clone(&novad.daemon.logger),
    );
    let nova_protocol::Event::AiUsage(usage) = again.event() else { panic!("not an ai_usage event") };
    assert_eq!(usage.shown, nova_protocol::AiProvider::Codex);
    assert_eq!(usage.providers[0].limits[0].percent, 42);
    let saved = std::fs::metadata(ai_usage_paths(&novad.dir.join("ai-usage")).state_file).unwrap();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&saved.permissions()) & 0o777, 0o600);
}

#[test]
fn a_window_drops_to_zero_when_it_resets() {
    let novad = TestDaemon::with_ai_usage("ai-usage-reset", |_| {});
    let mut client = novad.connect();
    client.subscribe_usage();
    let soon = (jiff::Timestamp::now() + jiff::SignedDuration::from_secs(1)).to_string();
    let limits = json!([{"window": "session", "percent": 97, "resets_at": soon}]);
    client.request(2, "report_claude_usage", Some(json!({"limits": limits})));
    assert_eq!(provider(&client.next_usage(), "claude")["limits"][0]["percent"], 97);
    let after = provider(&client.next_usage(), "claude").clone();
    assert_eq!(after["limits"][0], json!({"window": "session", "percent": 0}));
}

#[test]
fn bad_claude_reports_are_refused() {
    let novad = TestDaemon::with_ai_usage("ai-usage-bad", |_| {});
    let mut client = novad.connect();
    let report = |limits: Value| Some(json!({"limits": limits}));
    let over = client.request(1, "report_claude_usage", report(json!([{"window": "weekly", "percent": 101}])));
    assert!(over["error"].as_str().unwrap().contains("over 100"));
    let twice = json!([{"window": "weekly", "percent": 1}, {"window": "weekly", "percent": 2}]);
    assert!(client.request(2, "report_claude_usage", report(twice))["error"].as_str().unwrap().contains("twice"));
    let bad_time = json!([{"window": "weekly", "percent": 1, "resets_at": "soon"}]);
    assert!(client.request(3, "report_claude_usage", report(bad_time))["error"].is_string());
}

#[test]
fn ai_usage_commands_say_when_the_feature_is_off() {
    let novad = TestDaemon::start("ai-usage-off", None);
    let mut client = novad.connect();
    let reply = client.request(1, "show_ai_usage", Some(json!({"provider": "codex"})));
    assert_eq!(reply["error"], "the ai-usage feature is off");
    client.request(2, "subscribe", Some(json!({"topics": ["ai_usage"]})));
    assert_eq!(client.request(3, "ping", None)["result"], "pong", "no ai_usage event without the feature");
}

/// A compositor's sockets in a test's folder, serving what the real one sent
/// in `compositor/recorded/<name>`: its whole event stream at once and, for
/// Hyprland, its answers at the end of the recording. Every request is kept.
struct FakeCompositor {
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeCompositor {
    fn recorded(compositor: &str, file: &str) -> String {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compositor/recorded").join(compositor);
        std::fs::read_to_string(dir.join(file)).unwrap()
    }

    /// Answers each connection's one request with `respond`, then keeps the
    /// connection open until the client closes it, as an event stream is.
    fn answer(&self, socket: &Path, respond: impl Fn(&str) -> String + Send + Sync + 'static) {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let (requests, respond) = (Arc::clone(&self.requests), Arc::new(respond));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (requests, respond) = (Arc::clone(&requests), Arc::clone(&respond));
                std::thread::spawn(move || {
                    let mut stream = stream.unwrap();
                    // niri's requests end with a newline; Hyprland's clients stop writing.
                    let mut request = String::new();
                    BufReader::new(&stream).read_line(&mut request).unwrap();
                    let request = request.trim().to_owned();
                    let reply = respond(&request);
                    lock(&requests).push(request);
                    stream.write_all(reply.as_bytes()).unwrap();
                    let _ = std::io::copy(&mut stream, &mut std::io::sink());
                });
            }
        });
    }

    /// Sends `events` to each client that connects, without waiting for a
    /// request, and keeps the connection open.
    fn stream(socket: &Path, events: String) {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                stream.write_all(events.as_bytes()).unwrap();
                std::thread::spawn(move || std::io::copy(&mut stream, &mut std::io::sink()));
            }
        });
    }

    fn niri(dir: &Path) -> (Self, Box<dyn compositor::Backend>) {
        let fake = Self { requests: Arc::default() };
        let events = Self::recorded("niri", "events.txt");
        let steps: Vec<Value> = serde_json::from_str(&Self::recorded("niri", "steps.json")).unwrap();
        let windows = Self::recorded("niri", &format!("{:02}/windows.json", steps.len() - 1));
        let outputs = Self::recorded("niri", &format!("{:02}/outputs.json", steps.len() - 1));
        let outputs = format!("{}\n", json!({"Ok": {"Outputs": serde_json::from_str::<Value>(&outputs).unwrap()}}));
        let socket = dir.join("niri.sock");
        fake.answer(&socket, move |request| match request {
            // The recording starts with niri's answer to this.
            r#""EventStream""# => events.clone(),
            r#""Windows""# => format!("{{\"Ok\":{{\"Windows\":{}}}}}\n", windows.trim()),
            r#""Outputs""# => outputs.clone(),
            _ => "{\"Ok\":\"Handled\"}\n".into(),
        });
        let backend = compositor::detect(|name| (name == "NIRI_SOCKET").then(|| socket.clone().into())).unwrap();
        (fake, backend)
    }

    fn hyprland(dir: &Path) -> (Self, Box<dyn compositor::Backend>) {
        const INSTANCE: &str = "recorded";
        let fake = Self { requests: Arc::default() };
        let instance = dir.join("hypr").join(INSTANCE);
        std::fs::create_dir_all(&instance).unwrap();
        let steps: Vec<Value> = serde_json::from_str(&Self::recorded("hyprland", "steps.json")).unwrap();
        let last = format!("{:02}", steps.len() - 1);
        let events = Self::recorded("hyprland", "events.txt");
        Self::stream(&instance.join(".socket2.sock"), events);
        fake.answer(&instance.join(".socket.sock"), move |request| match request.strip_prefix("j/") {
            Some(query) => Self::recorded("hyprland", &format!("{last}/{query}.json")),
            None => "ok".into(),
        });
        let runtime_dir = dir.to_owned();
        let backend = compositor::detect(|name| match name {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some(INSTANCE.into()),
            "XDG_RUNTIME_DIR" => Some(runtime_dir.clone().into()),
            _ => None,
        })
        .unwrap();
        (fake, backend)
    }

    /// The requests that weren't queries or the event stream.
    fn actions(&self) -> Vec<String> {
        let queries = [r#""EventStream""#, r#""Windows""#, r#""Outputs""#];
        lock(&self.requests)
            .iter()
            .filter(|r| !r.starts_with("j/") && !queries.contains(&r.as_str()))
            .cloned()
            .collect()
    }
}

/// novad follows `fake` until its desktop has what the recording ends with:
/// the second layout, a workspace named `web` and no focused window.
fn follow_to_the_end(novad: &TestDaemon) -> (Client, Value) {
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["compositor"]})));
    loop {
        let desktop = client.next_event("compositor");
        let names: Vec<&str> = desktop["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|output| output["workspaces"].as_array().unwrap())
            .map(|workspace| workspace["name"].as_str().unwrap())
            .collect();
        if desktop["keyboard_layout"] == "German" && names.contains(&"web") && desktop.get("focused_window").is_none() {
            return (client, desktop);
        }
    }
}

fn workspace_id(desktop: &Value, name: &str) -> i64 {
    desktop["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["workspaces"].as_array().unwrap())
        .find(|workspace| workspace["name"] == name)
        .and_then(|workspace| workspace["id"].as_i64())
        .unwrap()
}

fn focused_output(desktop: &Value) -> String {
    let outputs = desktop["outputs"].as_array().unwrap();
    outputs.iter().find(|output| output["focused"] == true).unwrap()["name"].as_str().unwrap().to_owned()
}

#[test]
fn niri_reaches_subscribers_and_status_and_takes_workspace_actions() {
    let mut fake = None;
    let novad = TestDaemon::with_compositor("niri", |dir| {
        let (compositor, backend) = FakeCompositor::niri(dir);
        fake = Some(compositor);
        backend
    });
    let fake = fake.unwrap();
    let (mut client, desktop) = follow_to_the_end(&novad);
    assert_eq!(desktop["compositor"], "niri");
    let status = client.request(2, "status", None);
    assert_eq!(status["result"]["desktop"], desktop);

    let web = workspace_id(&desktop, "web");
    assert_eq!(client.request(3, "focus_workspace", Some(json!({"id": web})))["result"], Value::Null);
    let output = focused_output(&desktop);
    assert_eq!(
        client.request(4, "step_workspace", Some(json!({"output": output, "step": "next"})))["result"],
        Value::Null
    );
    let expected = |id: i64| json!({"Action": {"FocusWorkspace": {"reference": {"Id": id}}}}).to_string();
    assert_eq!(fake.actions(), [expected(web), expected(web)], "workspace 1 is shown, so next is web");

    let unknown = client.request(5, "focus_workspace", Some(json!({"id": 999})));
    assert!(unknown["error"].as_str().unwrap().contains("no workspace 999"), "{unknown}");
    assert_eq!(fake.actions().len(), 2, "unknown workspaces never reach niri");
}

#[test]
fn hyprland_reaches_subscribers_and_takes_workspace_actions() {
    let mut fake = None;
    let novad = TestDaemon::with_compositor("hyprland", |dir| {
        let (compositor, backend) = FakeCompositor::hyprland(dir);
        fake = Some(compositor);
        backend
    });
    let fake = fake.unwrap();
    let (mut client, desktop) = follow_to_the_end(&novad);
    assert_eq!(desktop["compositor"], "Hyprland");

    let web = workspace_id(&desktop, "web");
    assert_eq!(client.request(2, "focus_workspace", Some(json!({"id": web})))["result"], Value::Null);
    assert_eq!(fake.actions(), [format!("dispatch workspace {web}")]);
}

#[test]
fn compositor_actions_without_a_compositor_say_so() {
    let novad = TestDaemon::start("no-compositor", None);
    let mut client = novad.connect();
    let reply = client.request(1, "focus_workspace", Some(json!({"id": 1})));
    assert!(reply["error"].as_str().unwrap().contains("doesn't know this compositor"), "{reply}");
    assert_eq!(client.request(2, "status", None)["result"].get("desktop"), None);
}

#[test]
fn toggle_needs_the_panels_feature() {
    let novad = TestDaemon::start("toggle-off", None);
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["toggle"]})));
    let reply = novad.connect().request(1, "toggle", Some(json!({"panel": "ai-usage"})));
    assert!(reply["error"].as_str().unwrap().contains("ai-usage feature isn't ready"), "{reply}");
    assert_eq!(ui.request(2, "ping", None)["result"], "pong", "no toggle event without the feature");
}

#[test]
fn toggle_reaches_the_ui_once_it_listens() {
    let novad = TestDaemon::with_ai_usage("toggle", |_| {});
    let mut cli = novad.connect();
    let reply = cli.request(1, "toggle", Some(json!({"panel": "ai-usage"})));
    assert!(reply["error"].as_str().unwrap().contains("UI isn't connected"), "{reply}");

    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["toggle"]})));
    assert_eq!(cli.request(2, "toggle", Some(json!({"panel": "ai-usage"})))["result"], Value::Null);
    assert_eq!(ui.next_event("toggle"), json!({"panel": "ai-usage"}), "no output without a compositor");
    assert!(cli.request(3, "toggle", Some(json!({"panel": "calculator"})))["error"].is_string());
}

#[test]
fn toggle_names_the_focused_output() {
    let novad = TestDaemon::open_following("toggle-niri", None, |_| {}, |dir| Some(FakeCompositor::niri(dir).1));
    let (mut ui, desktop) = follow_to_the_end(&novad);
    ui.request(2, "subscribe", Some(json!({"topics": ["toggle"]})));
    let reply = novad.connect().request(1, "toggle", Some(json!({"panel": "ai-usage"})));
    assert_eq!(reply["result"], Value::Null);
    let toggle = ui.next_event("toggle");
    assert_eq!(toggle, json!({"panel": "ai-usage", "output": focused_output(&desktop)}));
}

#[test]
fn volume_keys_reach_the_ui_once_it_listens() {
    let novad = TestDaemon::start("volume", None);
    let mut cli = novad.connect();
    let reply = cli.request(1, "volume", Some(json!({"key": "up"})));
    assert!(reply["error"].as_str().unwrap().contains("UI isn't connected"), "{reply}");

    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["volume"]})));
    for (id, key) in [(2, "up"), (3, "down"), (4, "mute")] {
        assert_eq!(cli.request(id, "volume", Some(json!({"key": key})))["result"], Value::Null);
        assert_eq!(ui.next_event("volume"), json!({"key": key}));
    }
    assert!(cli.request(5, "volume", Some(json!({"key": "louder"})))["error"].is_string());
}

#[test]
fn lock_reaches_the_ui_and_a_restarted_ui_locks_again() {
    let novad = TestDaemon::start("lock", None);
    let mut cli = novad.connect();
    let reply = cli.request(1, "lock", None);
    assert!(reply["error"].as_str().unwrap().contains("UI isn't connected"), "{reply}");

    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    assert_eq!(ui.next_event("lock"), json!({"locked": false}), "the state on subscribe");
    // `nova lock` answers once the compositor has covered the screens.
    let locking = std::thread::spawn(move || cli.request(2, "lock", None));
    assert_eq!(ui.next_event("lock"), json!({"locked": true}));
    assert!(!locking.is_finished(), "answered before the screens were covered");
    assert_eq!(ui.request(2, "report_locked", None)["result"], Value::Null);
    assert_eq!(locking.join().unwrap()["result"], Value::Null);
    assert!(novad.dir.join("locked").exists(), "kept for a restarted novad");

    // A UI that crashed while locked hears on its restart that it should lock.
    drop(ui);
    let mut restarted = novad.connect();
    restarted.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    assert_eq!(restarted.next_event("lock"), json!({"locked": true}));
    // The screens stayed covered, so locking again answers at once.
    assert_eq!(novad.connect().request(1, "lock", None)["result"], Value::Null);

    assert_eq!(restarted.request(2, "report_unlocked", None)["result"], Value::Null);
    assert_eq!(restarted.next_event("lock"), json!({"locked": false}));
    assert!(!novad.dir.join("locked").exists());
}

#[test]
fn only_the_supervised_ui_reports_the_lock_screen() {
    let scripts = std::env::temp_dir().join(format!("novad-test-lock-ui-{}", std::process::id()));
    std::fs::create_dir_all(&scripts).unwrap();
    let fake_ui = scripts.join("quickshell");
    std::fs::write(&fake_ui, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&fake_ui, std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = UiSettings {
        quickshell: fake_ui,
        ui_dir: PathBuf::from("ui"),
        socket: PathBuf::from("unused.sock"),
        path_var: std::env::var("PATH").unwrap_or_default(),
        wayland_socket: None,
    };
    let novad = TestDaemon::start("lock-reports", Some(settings));
    let mut other = novad.connect();
    for report in ["report_locked", "report_unlocked"] {
        let reply = other.request(1, report, None);
        assert!(reply["error"].as_str().unwrap().contains("only the UI"), "{report}: {reply}");
    }
    novad.daemon.ui.as_ref().unwrap().stop();
    std::fs::remove_dir_all(scripts).unwrap();
}

#[test]
fn lock_fails_when_the_screens_arent_covered_in_time() {
    let novad = TestDaemon::start("lock-stalled", None);
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    // A UI that hears the lock but never covers the screens.
    let reply = novad.connect().request(1, "lock", None);
    assert!(reply["error"].as_str().unwrap().contains("weren't covered"), "{reply}");
}

#[test]
fn lock_needs_its_feature_but_unlocking_doesnt() {
    let novad = TestDaemon::open("lock-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"lock\"]\n").unwrap()
    });
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    let reply = novad.connect().request(1, "lock", None);
    assert!(reply["error"].as_str().unwrap().contains("lock feature isn't ready"), "{reply}");
    // Turned off while the screen was locked: the UI can still say it unlocked.
    assert_eq!(ui.request(2, "report_unlocked", None)["result"], Value::Null);
}

#[test]
fn power_options_list_every_action_say_why_one_cant_run_and_when_the_system_booted() {
    let novad = TestDaemon::start("power", None);
    let menu = novad.connect().request(1, "power_options", None)["result"].clone();
    assert!(menu["booted_at"].as_u64().is_some_and(|booted| booted > 0), "the uptime's start: {menu}");
    let options = menu["options"].clone();
    let actions: Vec<&str> =
        options.as_array().unwrap().iter().map(|option| option["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["lock", "suspend", "log_out", "reboot", "power_off"]);
    let lock = &options[0];
    assert_eq!(lock["available"], true, "the lock feature is on by default: {lock}");
    // No compositor here, so nothing to quit.
    let log_out = &options[2];
    assert_eq!(log_out["available"], false);
    assert!(log_out["reason"].as_str().unwrap().contains("Not on this compositor"), "{log_out}");
    for option in options.as_array().unwrap() {
        assert!(option["available"] == true || option["reason"].is_string(), "a dimmed action says why: {option}");
    }
}

#[test]
fn power_needs_its_feature_and_logs_out_through_the_compositor() {
    let novad = TestDaemon::open("power-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"power\"]\n").unwrap()
    });
    let mut cli = novad.connect();
    for (id, request) in [(1, json!(null)), (2, json!({"action": "lock"}))] {
        let command = if request.is_null() { "power_options" } else { "power" };
        let args = (!request.is_null()).then_some(request);
        let reply = cli.request(id, command, args);
        assert!(reply["error"].as_str().unwrap().contains("power feature isn't ready"), "{reply}");
    }

    let novad = TestDaemon::start("power-log-out", None);
    let reply = novad.connect().request(1, "power", Some(json!({"action": "log_out"})));
    assert!(reply["error"].as_str().unwrap().contains("doesn't know this compositor"), "{reply}");
}

#[test]
fn power_lock_locks_like_nova_lock() {
    let novad = TestDaemon::start("power-lock", None);
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    assert_eq!(ui.next_event("lock"), json!({"locked": false}));
    // The menu asks over the UI's own connection, which reports the screens
    // covered only after this answers: it doesn't wait for that.
    let asked = Instant::now();
    assert_eq!(ui.request(2, "power", Some(json!({"action": "lock"})))["result"], Value::Null);
    assert!(asked.elapsed() < Duration::from_secs(1), "waited {:?}", asked.elapsed());
    assert_eq!(ui.next_event("lock"), json!({"locked": true}));
    assert_eq!(ui.request(3, "report_locked", None)["result"], Value::Null);
    // Covered now, so `nova lock` answers at once.
    assert_eq!(novad.connect().request(1, "lock", None)["result"], Value::Null);
}

/// Saves `text` to `path` the way many editors do: a new file renamed over
/// the old one.
fn save_by_rename(path: &Path, text: &str) {
    let partial = path.with_extension("toml.new");
    std::fs::write(&partial, text).unwrap();
    std::fs::rename(partial, path).unwrap();
}

fn feature_status<'a>(features: &'a Value, id: &str) -> &'a Value {
    &features["features"].as_array().unwrap().iter().find(|feature| feature["id"] == id).unwrap()["status"]
}

#[test]
fn changing_the_theme_mode_recolors_live() {
    let novad = TestDaemon::start("reload-theme", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    let dark = client.next_event("theme");
    assert_eq!(dark["mode"], "dark");

    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    let light = client.next_event("theme");
    assert_eq!(light["mode"], "light");
    assert_ne!(light["colors"], dark["colors"]);

    save_by_rename(&novad.config_file(), "[theme]\nmode = \"dark\"\n");
    assert_eq!(client.next_event("theme"), dark);
}

#[test]
fn a_broken_config_keeps_the_last_good_one_and_says_why() {
    let novad = TestDaemon::start("reload-broken", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme", "logs"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");

    save_by_rename(&novad.config_file(), "[theme]\nmode = \"sepia\"\n");
    loop {
        let message = client.next();
        assert_ne!(message["event"], "theme", "a broken file changed the theme");
        // TOML's error spans several lines, each its own entry.
        let entries = message["data"]["entries"].as_array().cloned().unwrap_or_default();
        let error = entries
            .iter()
            .filter(|entry| entry["level"] == "error")
            .map(|entry| entry["message"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        if error.contains("sepia") && error.contains("keeping the last good ones") {
            break;
        }
    }
    let mut other = novad.connect();
    other.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(other.next_event("theme")["mode"], "light");

    std::fs::write(novad.config_file(), "[theme]\nmode = \"dark\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "dark");
}

#[test]
fn broken_files_at_start_mean_defaults() {
    let novad = TestDaemon::open("start-broken", None, |dir| {
        std::fs::write(dir.join("config/config.toml"), "[theme\n").unwrap();
        std::fs::write(dir.join("config/features.toml"), "enabled = 3\n").unwrap();
    });
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme", "features"]})));
    assert_eq!(client.next_event("theme")["id"], theme::DEFAULT_THEME_ID);
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "ready");
    let log = read_entries(&nova_paths::log_file(&novad.dir.join("logs")));
    let errors = log.iter().filter(|entry| {
        entry.level == LogLevel::Error && entry.message.starts_with("broken settings, using the defaults: ")
    });
    assert_eq!(errors.count(), 2, "{log:?}");
}

#[test]
fn turning_a_feature_off_and_on_stops_and_starts_it() {
    let novad = TestDaemon::with_ai_usage("reload-feature", |_| {});
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["features", "ai_usage"]})));
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "ready");
    client.next_usage();

    save_by_rename(&novad.features_file(), WATCHERS_OFF);
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "off");
    let reply = client.request(2, "show_ai_usage", Some(json!({"provider": "codex"})));
    assert_eq!(reply["error"], "the ai-usage feature is off");

    // No features.toml means each feature's default, which is on.
    std::fs::remove_file(novad.features_file()).unwrap();
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "ready");
    assert_eq!(client.next_usage()["shown"], "claude", "the restarted feature publishes again");
    let reply = client.request(3, "show_ai_usage", Some(json!({"provider": "codex"})));
    assert_eq!(reply["result"], Value::Null);
}

#[test]
fn turning_claudes_endpoint_on_applies_without_a_restart() {
    let (url, requests) = fake_endpoint(r#"{"five_hour":{"utilization":8.4,"resets_at":null}}"#);
    let novad = TestDaemon::with_ai_usage("reload-claude-endpoint", |root| claude_sign_in(root, &url));
    let mut client = novad.connect();
    assert_eq!(provider(&client.subscribe_usage(), "claude")["state"], "unavailable");
    assert!(requests.lock().unwrap().is_empty());

    std::fs::write(novad.config_file(), CLAUDE_ENDPOINT_ON).unwrap();
    while provider(&client.next_usage(), "claude")["state"] != "ready" {}
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn a_config_folder_made_after_start_is_watched() {
    let novad = TestDaemon::open("reload-new-folder", None, |dir| std::fs::remove_dir(dir.join("config")).unwrap());
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    std::fs::create_dir(novad.config_file().parent().unwrap()).unwrap();
    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");
}

#[test]
fn edits_through_a_symlinked_config_are_seen() {
    let novad = TestDaemon::open("reload-symlink", None, |dir| {
        std::fs::create_dir(dir.join("dotfiles")).unwrap();
        std::fs::write(dir.join("dotfiles/config.toml"), "[theme]\nmode = \"dark\"\n").unwrap();
        std::os::unix::fs::symlink(dir.join("dotfiles/config.toml"), dir.join("config/config.toml")).unwrap();
    });
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    std::fs::write(novad.dir.join("dotfiles/config.toml"), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");
}

#[test]
fn a_replaced_config_folder_is_followed() {
    let novad = TestDaemon::start("reload-replaced-folder", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    let folder = novad.config_file().parent().unwrap().to_owned();
    std::fs::rename(&folder, novad.dir.join("config.old")).unwrap();
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(novad.features_file(), WATCHERS_OFF).unwrap();
    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");
    // Edits in the new folder keep applying.
    std::fs::write(novad.config_file(), "[theme]\nmode = \"dark\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "dark");
}

#[test]
fn screenshots_need_their_feature_but_cancelling_doesnt() {
    let novad = TestDaemon::open("screenshot-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"screenshot\"]\n").unwrap()
    });
    let mut cli = novad.connect();
    for (id, mode) in [(1, "region"), (2, "screen")] {
        let reply = cli.request(id, "screenshot", Some(json!({"mode": mode})));
        assert!(reply["error"].as_str().unwrap().contains("screenshot feature isn't ready"), "{reply}");
    }
    let rect = json!({"output": "DP-1", "rect": {"x": 0, "y": 0, "width": 1, "height": 1}});
    assert!(cli.request(3, "save_capture", Some(rect))["error"].is_string());
    assert_eq!(cli.request(4, "cancel_capture", None)["result"], Value::Null);
}

#[test]
fn recording_needs_its_feature_but_stopping_doesnt() {
    let novad = TestDaemon::open("recording-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"recording\"]\n").unwrap()
    });
    let mut cli = novad.connect();
    let reply = cli.request(1, "record", None);
    assert!(reply["error"].as_str().unwrap().contains("recording feature isn't ready"), "{reply}");
    let reply = cli.request(2, "start_recording", Some(json!({"output": "DP-1", "region": null})));
    assert!(reply["error"].as_str().unwrap().contains("recording feature isn't ready"), "{reply}");
    let reply = cli.request(3, "stop_recording", None);
    assert!(reply["error"].as_str().unwrap().contains("nothing is being recorded"), "{reply}");
    cli.request(4, "subscribe", Some(json!({"topics": ["recording"]})));
    assert_eq!(cli.next_event("recording"), Value::Null, "nothing is being recorded");
}

#[test]
fn subscribing_to_host_sends_the_machine_and_its_user() {
    let novad = TestDaemon::start("host", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["host"]})));
    let host = client.next_event("host");
    assert!(host["booted_at"].as_i64().is_some_and(|booted| booted > 0));
    assert!(host["user"].as_str().is_some_and(|user| !user.is_empty()));
    assert!(host["os"]["id"].is_string());

    std::fs::write(novad.config_file(), "[quick-settings]\nname = \"Katov\"\n").unwrap();
    assert_eq!(client.next_event("host")["user"], "Katov", "the config's name applies live");
}

/// Resources come only while a connection watches, and stop once the last
/// one stops watching or hangs up.
#[test]
fn resources_are_sent_while_watched() {
    let novad = TestDaemon::start("resources", None);
    let mut listener = novad.connect();
    listener.request(1, "subscribe", Some(json!({"topics": ["resources"]})));
    let mut watcher = novad.connect();
    assert_eq!(watcher.request(1, "watch_resources", Some(json!({"watching": true})))["result"], Value::Null);
    let reading = listener.next_event("resources");
    assert!(reading["memory_total"].as_u64().is_some_and(|total| total > 0));
    assert!(reading["apps"].as_array().is_some_and(|apps| !apps.is_empty()));
    let nova = reading["apps"].as_array().unwrap().iter().find(|app| app["key"] == "nova");
    assert!(nova.is_some_and(|nova| nova["name"] == "Nova" && nova["protected"] == true), "Nova is listed: {reading}");
    assert!(reading["history"].is_array());

    drop(watcher);
    // Looks already sent when it hung up arrive first; then nothing.
    std::thread::sleep(resources::LIVE_INTERVAL);
    listener.writer.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    while listener.lines.next().is_some_and(|line| line.is_ok()) {}
    listener.writer.set_read_timeout(Some(resources::LIVE_INTERVAL * 2)).unwrap();
    let late = listener.lines.next();
    assert!(late.as_ref().is_none_or(Result::is_err), "still sent after the watcher left: {late:?}");
}

/// Ending an app only ends what Nova offers to: not Nova itself, and not
/// what isn't running.
#[test]
fn ending_nova_or_a_missing_app_is_refused() {
    let novad = TestDaemon::start("end-app", None);
    let mut client = novad.connect();
    let reply = client.request(1, "end_app", Some(json!({"key": "nova"})));
    assert!(reply["error"].as_str().is_some_and(|error| error.contains("can't be ended")), "{reply}");
    let reply = client.request(2, "end_app", Some(json!({"key": "/nowhere/app"})));
    assert!(reply["error"].as_str().is_some_and(|error| error.contains("no app")), "{reply}");
}

#[test]
fn watching_resources_needs_the_feature() {
    let novad = TestDaemon::open("resources-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"quick-settings\"]\n").unwrap();
    });
    let mut client = novad.connect();
    let reply = client.request(1, "watch_resources", Some(json!({"watching": true})));
    assert!(reply["error"].as_str().is_some_and(|error| error.contains("quick-settings")), "{reply}");
}

/// Writes an app's desktop entry where the test daemon finds apps.
fn install_app(novad: &TestDaemon, file: &str, entry: &str) {
    let path = novad.dir.join("launcher/share/applications").join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Written beside it and moved in, as package managers do.
    let partial = path.with_extension("partial");
    std::fs::write(&partial, format!("[Desktop Entry]\nType=Application\n{entry}")).unwrap();
    std::fs::rename(partial, path).unwrap();
}

impl Client {
    /// Subscribes to the launcher and returns its data once it lists `apps` apps.
    fn subscribe_launcher(&mut self, apps: usize) -> Value {
        self.request(1, "subscribe", Some(json!({"topics": ["launcher"]})));
        let first = self.next_event("launcher");
        match first["apps"].as_array().unwrap().len() == apps {
            true => first,
            false => self.launcher_until(|event| event["apps"].as_array().unwrap().len() == apps),
        }
    }

    /// The next launcher event that `done` accepts.
    fn launcher_until(&mut self, done: impl Fn(&Value) -> bool) -> Value {
        loop {
            let event = self.next_event("launcher");
            if done(&event) {
                return event;
            }
        }
    }
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array().unwrap().iter().map(|item| item.as_str().unwrap_or_else(|| item["id"].as_str().unwrap())).collect()
}

#[test]
fn the_launcher_lists_apps_and_follows_installs() {
    let novad = TestDaemon::with_launcher("launcher-apps");
    install_app(&novad, "foot.desktop", "Name=Foot\nGenericName=Terminal\nIcon=foot\nExec=foot\n");
    install_app(&novad, "hidden.desktop", "Name=Hidden\nNoDisplay=true\nExec=x\n");
    let mut client = novad.connect();
    let first = client.subscribe_launcher(1);
    assert_eq!(
        first["apps"],
        json!([{"id": "foot.desktop", "name": "Foot", "description": "Terminal", "icon": "foot"}])
    );

    // A new app, in a folder made after novad started, shows up.
    install_app(&novad, "org.kde/dolphin.desktop", "Name=Dolphin\nExec=dolphin %u\n");
    let event = client.launcher_until(|event| event["apps"].as_array().unwrap().len() == 2);
    assert_eq!(ids(&event["apps"]), ["org.kde-dolphin.desktop", "foot.desktop"], "sorted by name");
    // And one that's removed goes.
    std::fs::remove_file(novad.dir.join("launcher/share/applications/foot.desktop")).unwrap();
    client.launcher_until(|event| ids(&event["apps"]) == ["org.kde-dolphin.desktop"]);

    // A whole new applications folder renamed in place of the old one is
    // read, and so are apps installed into it afterwards.
    let applications = novad.dir.join("launcher/share/applications");
    let replacement = novad.dir.join("launcher/share/applications.new");
    std::fs::create_dir_all(&replacement).unwrap();
    std::fs::write(replacement.join("a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=a\n").unwrap();
    std::fs::rename(&applications, novad.dir.join("launcher/share/applications.old")).unwrap();
    std::fs::rename(&replacement, &applications).unwrap();
    client.launcher_until(|event| ids(&event["apps"]) == ["a.desktop"]);
    install_app(&novad, "b.desktop", "Name=B\nExec=b\n");
    client.launcher_until(|event| ids(&event["apps"]) == ["a.desktop", "b.desktop"]);
    // Moved away with nothing in its place, its apps go.
    std::fs::rename(&applications, novad.dir.join("launcher/share/gone")).unwrap();
    client.launcher_until(|event| event["apps"].as_array().unwrap().is_empty());
}

#[test]
fn the_launcher_follows_apps_behind_a_linked_subfolder() {
    let novad = TestDaemon::with_launcher("launcher-linked");
    let vendor = novad.dir.join("vendor-apps");
    std::fs::create_dir_all(&vendor).unwrap();
    install_app(&novad, "foot.desktop", "Name=Foot\nExec=foot\n");
    std::os::unix::fs::symlink(&vendor, novad.dir.join("launcher/share/applications/vendor")).unwrap();
    let mut client = novad.connect();
    client.subscribe_launcher(1);
    std::fs::write(vendor.join("a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=a\n").unwrap();
    client.launcher_until(|event| ids(&event["apps"]) == ["vendor-a.desktop", "foot.desktop"]);
}

#[test]
fn launcher_searches_rank_apps_and_refuse_long_queries() {
    let novad = TestDaemon::with_launcher("launcher-search");
    install_app(&novad, "firefox.desktop", "Name=Firefox\nGenericName=Web Browser\nExec=firefox\n");
    install_app(&novad, "files.desktop", "Name=Files\nKeywords=folder;\nExec=nautilus\n");
    install_app(&novad, "fooview.desktop", "Name=Fooview\nExec=fooview\n");
    let mut client = novad.connect();
    client.subscribe_launcher(3);

    let found = client.request(2, "search_apps", Some(json!({"query": " FI "})));
    // Fooview has f and i in order, which counts least.
    assert_eq!(ids(&found["result"]), ["files.desktop", "firefox.desktop", "fooview.desktop"]);
    assert_eq!(found["result"][0]["name"], json!([{"text": "Fi", "matched": true}, {"text": "les"}]));
    let by_keyword = client.request(3, "search_apps", Some(json!({"query": "folder"})));
    assert_eq!(by_keyword["result"], json!([{"id": "files.desktop", "name": [{"text": "Files"}]}]));
    assert_eq!(client.request(4, "search_apps", Some(json!({"query": ""})))["result"], json!([]));
    assert_eq!(client.request(5, "search_apps", Some(json!({"query": "zzz"})))["result"], json!([]));
    let long = client.request(6, "search_apps", Some(json!({"query": "x".repeat(201)})));
    assert!(long["error"].as_str().unwrap().contains("at most 200"));
}

#[test]
fn launching_runs_the_app_without_a_shell_and_makes_it_recent() {
    let novad = TestDaemon::with_launcher("launcher-launch");
    let marker = novad.dir.join("launched");
    // %U is dropped; the semicolon would only end a command in a shell, and
    // lands in Path= as a file.
    let entry = format!("Name=Toucher\nExec=touch \"{}\" %U ;\nPath={}\n", marker.display(), novad.dir.display());
    install_app(&novad, "touch.desktop", &entry);
    // cp copies what the kernel says about it: its session.
    let stat = novad.dir.join("stat");
    install_app(&novad, "other.desktop", &format!("Name=Other\nExec=cp /proc/self/stat \"{}\"\n", stat.display()));
    let mut client = novad.connect();
    client.subscribe_launcher(2);

    assert_eq!(client.request(2, "launch_app", Some(json!({"id": "touch.desktop"})))["result"], Value::Null);
    let event = client.launcher_until(|event| !event["recent"].as_array().unwrap().is_empty());
    assert_eq!(event["recent"], json!(["touch.desktop"]));
    let ran = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    });
    assert!(ran, "the app didn't run");
    assert!(novad.dir.join(";").exists(), "`;` reached touch as a file name, not a shell");

    // No Path= and a HOME that may not exist (Nix's builds): it starts in `/`.
    assert_eq!(client.request(3, "launch_app", Some(json!({"id": "other.desktop"})))["result"], Value::Null);
    let event = client.launcher_until(|event| event["recent"].as_array().unwrap().len() == 2);
    assert_eq!(event["recent"], json!(["other.desktop", "touch.desktop"]), "the latest first");
    // Another session, away from novad's terminal (an app touching it would
    // be stopped as a background job): stat's 6th field, after the name.
    let copied = (0..100).find_map(|_| {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::read_to_string(&stat).ok().filter(|text| text.ends_with('\n'))
    });
    let copied = copied.expect("the app didn't run");
    let fields: Vec<&str> = copied.rsplit_once(") ").unwrap().1.split(' ').collect();
    // SAFETY: getsid only reads.
    let ours = unsafe { libc::getsid(0) }.to_string();
    assert_ne!(fields[3], ours, "in novad's session: {copied}");
    let missing = client.request(4, "launch_app", Some(json!({"id": "nothing.desktop"})));
    assert!(missing["error"].as_str().unwrap().contains("no app"));
}

#[test]
fn a_launched_app_outlives_the_request_that_started_it() {
    let novad = TestDaemon::with_launcher("launcher-outlives");
    // Like bubblewrap's --die-with-parent (Claude Desktop): killed when its
    // parent thread ends, here the one that answered the request.
    let pid_file = novad.dir.join("pid");
    let script = novad.dir.join("app.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho $$ > \"{}\"\nexec sleep 30\n", pid_file.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    install_app(
        &novad,
        "doomed.desktop",
        &format!("Name=Doomed\nExec=setpriv --pdeathsig KILL {}\n", script.display()),
    );
    let mut client = novad.connect();
    client.subscribe_launcher(1);
    assert_eq!(client.request(2, "launch_app", Some(json!({"id": "doomed.desktop"})))["result"], Value::Null);
    let pid = (0..100).find_map(|_| {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::read_to_string(&pid_file).ok().and_then(|text| text.trim().parse::<i32>().ok())
    });
    let pid = pid.expect("the app didn't run");
    drop(client);
    std::thread::sleep(Duration::from_millis(500));
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let field = |name: &str| {
        status.lines().find_map(|line| line.strip_prefix(name)).map(str::trim).unwrap_or_default().to_owned()
    };
    let alive = !status.is_empty() && !field("State:").starts_with('Z');
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    assert!(alive, "the app died with the thread that started it");
    assert_ne!(field("PPid:"), std::process::id().to_string(), "the app is novad's child");
}

/// Launches the app the recording's window belongs to (`nova-second`, on
/// workspace `web`) from the focused output, and returns the actions taken
/// and whether the app ran anyway, then the same asking for a new window.
fn bring_the_recorded_window(
    name: &str,
    fake: fn(&Path) -> (FakeCompositor, Box<dyn compositor::Backend>),
) -> (Vec<String>, Value, bool) {
    let mut compositor = None;
    let novad = TestDaemon::with_launcher_following(name, |dir| {
        let (fake, backend) = fake(dir);
        compositor = Some(fake);
        backend
    });
    let compositor = compositor.unwrap();
    let marker = novad.dir.join("ran");
    let entry = format!("Name=Second\nExec=touch \"{}\"\nStartupWMClass=Nova-Second\n", marker.display());
    install_app(&novad, "second.desktop", &entry);
    let (mut client, desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(1);
    let output = focused_output(&desktop);
    let launch = json!({"id": "second.desktop", "output": output});
    assert_eq!(client.request(2, "launch_app", Some(launch))["result"], Value::Null);
    let event = client.launcher_until(|event| !event["recent"].as_array().unwrap().is_empty());
    assert_eq!(event["recent"], json!(["second.desktop"]), "bringing it counts as a launch");
    let actions = compositor.actions();

    let fresh = json!({"id": "second.desktop", "output": output, "new_window": true});
    assert_eq!(client.request(3, "launch_app", Some(fresh))["result"], Value::Null);
    let ran = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    });
    assert_eq!(compositor.actions(), actions, "a new window leaves the open one alone");
    let shown =
        desktop["outputs"].as_array().unwrap().iter().find(|o| o["name"] == output.as_str()).unwrap()["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|workspace| workspace["active"] == true)
            .unwrap()
            .clone();
    (actions, shown, ran)
}

#[test]
fn launching_an_open_app_on_niri_brings_its_window_to_the_screen() {
    let (actions, shown, ran) = bring_the_recorded_window("launcher-bring-niri", FakeCompositor::niri);
    let id = shown["id"].as_i64().unwrap();
    let moved = json!({"Action": {"MoveWindowToWorkspace": {"window_id": 3, "reference": {"Id": id}, "focus": true}}});
    let focused = json!({"Action": {"FocusWindow": {"id": 3}}});
    assert_ne!(shown["name"], "web", "the recording shows another workspace there");
    assert_eq!(actions, [moved.to_string(), focused.to_string()]);
    assert!(ran, "asked for a new window, the app runs");
}

#[test]
fn launching_an_open_app_on_hyprland_brings_its_window_to_the_screen() {
    let (actions, shown, ran) = bring_the_recorded_window("launcher-bring-hyprland", FakeCompositor::hyprland);
    let id = shown["id"].as_i64().unwrap();
    assert_ne!(shown["name"], "web", "the recording shows another workspace there");
    let steps: Vec<Value> = serde_json::from_str(&FakeCompositor::recorded("hyprland", "steps.json")).unwrap();
    let clients = FakeCompositor::recorded("hyprland", &format!("{:02}/clients.json", steps.len() - 1));
    let clients: Value = serde_json::from_str(&clients).unwrap();
    let second = clients.as_array().unwrap().iter().find(|client| client["class"] == "nova-second").unwrap();
    let address = second["address"].as_str().unwrap();
    assert_eq!(
        actions,
        [
            format!("dispatch movetoworkspacesilent {id},address:{address}"),
            format!("dispatch focuswindow address:{address}")
        ]
    );
    assert!(ran, "asked for a new window, the app runs");
}

#[test]
fn pins_keep_their_order_and_survive_a_restart() {
    let novad = TestDaemon::with_launcher("launcher-pins");
    for name in ["a", "b", "c"] {
        install_app(&novad, &format!("{name}.desktop"), &format!("Name={name}\nExec=true\n"));
    }
    let mut client = novad.connect();
    client.subscribe_launcher(3);
    for (id, name) in [(2, "c"), (3, "a"), (4, "b")] {
        client.request(id, "pin_app", Some(json!({"id": format!("{name}.desktop"), "pinned": true})));
    }
    client.request(5, "pin_app", Some(json!({"id": "a.desktop", "pinned": false})));
    let event = client.launcher_until(|event| ids(&event["pinned"]) == ["c.desktop", "b.desktop"]);
    assert_eq!(ids(&event["pinned"]).len(), 2);
    let unknown = client.request(6, "pin_app", Some(json!({"id": "../../etc/passwd", "pinned": true})));
    assert!(unknown["error"].as_str().unwrap().contains("no app"));

    // A pinned app that's uninstalled isn't sent, but comes back with it.
    std::fs::remove_file(novad.dir.join("launcher/share/applications/c.desktop")).unwrap();
    client.launcher_until(|event| ids(&event["pinned"]) == ["b.desktop"]);
    install_app(&novad, "c.desktop", "Name=c\nExec=true\n");
    client.launcher_until(|event| ids(&event["pinned"]) == ["c.desktop", "b.desktop"]);

    let again = launcher::Launcher::start(
        launcher_paths(&novad.dir),
        "",
        Arc::new(Hub::default()),
        Arc::clone(&novad.daemon.logger),
    );
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(launcher_paths(&novad.dir).state_file).unwrap()).unwrap();
    assert_eq!(saved["pinned"], json!(["c.desktop", "b.desktop"]));
    again.stop();
}

#[test]
fn the_launcher_stops_with_its_feature() {
    let novad = TestDaemon::with_launcher("launcher-off");
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["features"]})));
    client.next_event("features");
    std::fs::write(novad.features_file(), "disabled = [\"ai-usage\", \"launcher\"]\n").unwrap();
    loop {
        let features = client.next_event("features");
        let launcher = features["features"].as_array().unwrap().iter().find(|f| f["id"] == "launcher").cloned();
        if launcher.is_some_and(|launcher| launcher["status"] == "off") {
            break;
        }
    }
    let off = client.request(2, "search_apps", Some(json!({"query": "a"})));
    assert!(off["error"].as_str().unwrap().contains("feature is off"));
    let toggle = client.request(3, "toggle", Some(json!({"panel": "launcher"})));
    assert!(toggle["error"].as_str().unwrap().contains("isn't ready"));
}
