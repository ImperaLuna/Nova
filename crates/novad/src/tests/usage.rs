//! AI usage: Claude and Codex limits from their files and endpoints.

use super::*;

/// `config.toml` with Claude's usage endpoint turned on.
pub(super) const CLAUDE_ENDPOINT_ON: &str = "[ai-usage]\nclaude-usage-endpoint = true\n";
/// `config.toml` with Codex's usage endpoint turned on.
const CODEX_ENDPOINT_ON: &str = "[ai-usage]\ncodex-usage-endpoint = true\n";

/// Answers every request with `body`, keeping the request heads it got.
pub(super) fn fake_endpoint(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
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
pub(super) fn claude_sign_in(root: &Path, url: &str) {
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
    pub(super) fn next_usage(&mut self) -> Value {
        self.next_event("ai_usage")
    }

    pub(super) fn subscribe_usage(&mut self) -> Value {
        self.request(1, "subscribe", Some(json!({"topics": ["ai_usage"]})));
        let mut usage = self.next_usage();
        while usage["providers"].as_array().unwrap().iter().any(|p| p["state"] == "loading") {
            usage = self.next_usage();
        }
        usage
    }
}

pub(super) fn provider<'a>(usage: &'a Value, id: &str) -> &'a Value {
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
fn claude_limits_use_the_desktop_apps_sign_in_when_claude_code_has_none() {
    let (url, requests) = fake_endpoint(r#"{"five_hour":{"utilization":4.0,"resets_at":"2100-01-01T05:00:00Z"}}"#);
    let novad = TestDaemon::with_ai_usage("ai-usage-claude-desktop", |root| {
        claude_sign_in(root, &url);
        // What Claude Code leaves when the desktop app signs it in.
        let empty = r#"{"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0,"subscriptionType":"max"}}"#;
        std::fs::write(root.join("claude/.credentials.json"), empty).unwrap();
        let expires = jiff::Timestamp::now().as_millisecond() + 3_600_000;
        let cache = json!({
            ai_usage::claude_desktop_entry("user:inference user:profile"):
                {"token": "app-token", "expiresAt": expires, "subscriptionType": "pro"}
        });
        ai_usage::write_claude_desktop_config(&root.join("claude-desktop"), &cache);
        std::fs::write(root.join("../config/config.toml"), CLAUDE_ENDPOINT_ON).unwrap();
    });
    let claude = provider(&novad.connect().subscribe_usage(), "claude").clone();
    assert_eq!((claude["state"].as_str(), claude["plan"].as_str()), (Some("ready"), Some("Pro")), "{claude}");
    assert_eq!(claude["limits"][0]["percent"], 4);
    let head = requests.lock().unwrap()[0].to_lowercase();
    assert!(head.contains("authorization: bearer app-token\r\n"), "{head}");
}

#[test]
fn reset_numbers_give_way_to_why_there_are_none() {
    let novad = TestDaemon::with_ai_usage("ai-usage-claude-stale", |root| {
        // Numbers from a status line whose windows have all reset since.
        let saved = json!({"claude": [
            {"window": "session", "percent": 1, "resets_at": "2026-10-30T17:59:59Z"},
            {"window": "weekly", "percent": 8, "resets_at": "2026-10-31T21:00:00Z"}
        ], "claude_plan": "Max"});
        std::fs::create_dir_all(root.join("state")).unwrap();
        std::fs::write(root.join("state/ai-usage.json"), saved.to_string()).unwrap();
        std::fs::write(root.join("../config/config.toml"), CLAUDE_ENDPOINT_ON).unwrap();
    });
    let mut client = novad.connect();
    let mut usage = client.subscribe_usage();
    // Once the endpoint has said why: the 0% before it is a guess.
    while provider(&usage, "claude")["state"] != "unavailable" {
        usage = client.next_usage();
    }
    assert_eq!(provider(&usage, "claude")["hint"], "Signed out of Claude Code");
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
    let again = nova_ai_usage::Usage::start(
        ai_usage_paths(&novad.dir.join("ai-usage")),
        &config::AiUsageConfig::default(),
        Arc::new(Hub::default()),
        Arc::clone(&novad.daemon.logger),
    );
    let nova_ai_usage_api::Event::AiUsage(usage) = again.event();
    assert_eq!(usage.shown, nova_ai_usage_api::AiProvider::Codex);
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
    assert_eq!(reply["error"], "the ai-usage feature isn't ready (see `nova status`)");
    client.request(2, "subscribe", Some(json!({"topics": ["ai_usage"]})));
    assert_eq!(client.request(3, "ping", None)["result"], "pong", "no ai_usage event without the feature");
}
