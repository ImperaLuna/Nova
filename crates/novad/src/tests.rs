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
        Self::open(name, ui, |_| None)
    }

    /// With the `ai-usage` feature on, its files under `<dir>/ai-usage`.
    fn with_ai_usage(name: &str, prepare: impl FnOnce(&Path)) -> Self {
        Self::open(name, None, |dir| {
            let root = dir.join("ai-usage");
            std::fs::create_dir_all(&root).unwrap();
            prepare(&root);
            Some(ai_usage_paths(&root))
        })
    }

    fn open(name: &str, ui: Option<UiSettings>, ai_usage: impl FnOnce(&Path) -> Option<ai_usage::Paths>) -> Self {
        let dir = std::env::temp_dir().join(format!("novad-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let socket = dir.join("novad.sock");
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let listener = server::bind(&socket).unwrap();
        let ui = ui.map(|settings| Supervisor::start(settings, Arc::clone(&logger)));
        let features = vec![FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] }];
        let (theme, _) = theme::resolve(&theme::default_dir(), &dir.join("no-config.toml"));
        let ai_usage = ai_usage(&dir).map(|paths| ai_usage::Usage::start(paths, Arc::clone(&hub), Arc::clone(&logger)));
        let daemon = Arc::new(Daemon {
            started: Instant::now(),
            features,
            theme,
            hub,
            logger,
            ui,
            socket: socket.clone(),
            ai_usage,
        });
        let serving = Arc::clone(&daemon);
        std::thread::spawn(move || server::serve(listener, serving));
        Self { dir, socket, daemon }
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

/// Claude's endpoint is `claude_usage_url`; nothing listens there unless a
/// test starts a fake one.
fn ai_usage_paths(root: &Path) -> ai_usage::Paths {
    ai_usage::Paths {
        codex_home: Some(root.join("codex")),
        state_file: root.join("state/ai-usage.json"),
        claude_credentials: Some(root.join("claude/.credentials.json")),
        claude_usage_url: std::fs::read_to_string(root.join("claude/url")).unwrap_or("http://127.0.0.1:9/".into()),
    }
}

/// Answers every request with `body`, keeping the request heads it got.
fn fake_anthropic(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/oauth/usage", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut head = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while reader.read_line(&mut head).unwrap() > 2 {}
            seen.lock().unwrap().push(head);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (url, requests)
}

/// Claude Code's credentials file, signed in to a Max plan.
fn claude_sign_in(root: &Path, url: &str) {
    let dir = root.join("claude");
    std::fs::create_dir_all(&dir).unwrap();
    let expires = jiff::Timestamp::now().as_millisecond() + 3_600_000;
    let credentials =
        format!(r#"{{"claudeAiOauth":{{"accessToken":"tok","expiresAt":{expires},"subscriptionType":"max"}}}}"#);
    std::fs::write(dir.join(".credentials.json"), credentials).unwrap();
    std::fs::write(dir.join("url"), url).unwrap();
}

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
    /// The next `ai_usage` event, skipping replies; fails after a while.
    fn next_usage(&mut self) -> Value {
        self.writer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        loop {
            let message = match self.early.pop_front() {
                Some(event) => event,
                None => {
                    let line = self.lines.next().expect("connection closed").expect("no ai_usage event in time");
                    serde_json::from_str(&line).unwrap()
                }
            };
            if message["event"] == "ai_usage" {
                return message["data"].clone();
            }
        }
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
    assert!(claude["hint"].as_str().unwrap().contains("Sign in to Claude Code"), "{claude}");
    assert_eq!(provider(&usage, "codex")["state"], "unavailable");
}

#[test]
fn claude_limits_come_from_anthropic_with_claude_codes_sign_in() {
    let (url, requests) = fake_anthropic(
        r#"{"five_hour":{"utilization":8.4,"resets_at":"2100-01-01T05:00:00.451561+00:00"},"seven_day":{"utilization":17.0,"resets_at":"2100-01-07T00:00:00+00:00"},"seven_day_opus":null}"#,
    );
    let novad = TestDaemon::with_ai_usage("ai-usage-claude-api", |root| claude_sign_in(root, &url));
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

    // Opening the panel right after the first answer doesn't ask again.
    client.request(2, "refresh_ai_usage", None);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(requests.lock().unwrap().len(), 1);
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
    let codex = provider(&client.next_usage(), "codex").clone();
    assert_eq!(codex["limits"][0]["percent"], 13);
}

#[test]
fn codex_sessions_created_later_are_picked_up() {
    let mut root_dir = PathBuf::new();
    let novad = TestDaemon::with_ai_usage("ai-usage-codex-later", |root| {
        std::fs::create_dir_all(root.join("codex")).unwrap();
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
        Arc::new(Hub::default()),
        Arc::clone(&novad.daemon.logger),
    );
    let nova_protocol::Event::AiUsage(usage) = again.event() else { panic!("not an ai_usage event") };
    assert_eq!(usage.shown, nova_protocol::AiProvider::Codex);
    assert_eq!(usage.providers[0].limits[0].percent, 42);
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
