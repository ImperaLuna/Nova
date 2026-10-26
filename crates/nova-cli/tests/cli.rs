//! Integration tests: the real `nova` binary against a real daemon (in this
//! process, without the UI) over a real socket.

use nova_protocol::{Status, UiState};
use nova_test_support::TempDir;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// `nova daemon`'s arguments, parsed on their own.
#[derive(clap::Parser)]
struct DaemonArgs {
    #[command(flatten)]
    args: novad::Args,
}

/// novad in this process, stopped when dropped, then its folder removed.
struct Novad {
    // Before `dir`: fields drop in order.
    _running: novad::InProcess,
    dir: TempDir,
}

impl Novad {
    fn start(name: &str) -> Self {
        Self::start_with(name, "")
    }

    /// With `selection` as the user's features.toml.
    fn start_with(name: &str, selection: &str) -> Self {
        let dir = TempDir::new(name);
        std::fs::write(dir.join("features.toml"), selection).unwrap();
        let features = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let args: Vec<std::ffi::OsString> = vec![
            "nova daemon".into(),
            "--no-ui".into(),
            "--log-dir".into(),
            dir.join("logs").into(),
            "--features-dir".into(),
            features.into(),
            "--selection".into(),
            dir.join("features.toml").into(),
            "--config".into(),
            dir.join("config.toml").into(),
            "--state-dir".into(),
            dir.join("state").into(),
            "--codex-home".into(),
            dir.join("codex").into(),
            "--claude-credentials".into(),
            dir.join("claude/.credentials.json").into(),
        ];
        let running =
            novad::spawn(<DaemonArgs as clap::Parser>::parse_from(args).args, dir.join("novad.sock")).unwrap();
        Self { _running: running, dir }
    }

    fn nova(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nova"));
        // Before the command, so it never ends up after a `--`.
        command.arg("--socket").arg(self.dir.join("novad.sock")).args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.nova(args).output().unwrap()
    }

    fn log_dir(&self) -> String {
        self.dir.join("logs").display().to_string()
    }
}

fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn status_json_is_novads_answer() {
    let novad = Novad::start("status-json");
    let status: Status = serde_json::from_str(&stdout(&novad.run(&["status", "--json"]))).unwrap();
    assert_eq!(status.ui.state, UiState::Disabled);
    assert!(status.features.iter().any(|f| f.id == "core"));
}

#[test]
fn nova_shows_status_when_already_running() {
    let novad = Novad::start("already-running");
    let text = stdout(&novad.run(&[]));
    assert!(text.starts_with("Nova is already running. To restart it, run `nova restart`."), "{text}");
    assert!(text.contains("daemon     running"), "{text}");
    let features = text.lines().find(|line| line.starts_with("features")).unwrap_or_default();
    assert!(features.contains("core on"), "{text}");
}

#[test]
fn version_is_one_line_when_novad_matches() {
    let novad = Novad::start("version");
    let text = stdout(&novad.run(&["version"]));
    assert_eq!(text, format!("nova {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn logs_print_history_from_the_file() {
    let novad = Novad::start("logs-file");
    // No --log-dir: the running novad says where its log is.
    let text = stdout(&novad.run(&["logs", "--no-follow"]));
    assert!(text.contains("listening on"), "{text}");
    let ui_only = stdout(&novad.run(&["logs", "--no-follow", "--ui", "--log-dir", &novad.log_dir()]));
    assert_eq!(ui_only, "", "novad runs without a UI here");
    let warnings = stdout(&novad.run(&["logs", "--no-follow", "--level", "warn", "--log-dir", &novad.log_dir()]));
    assert!(!warnings.contains("listening on"), "{warnings}");
}

#[test]
fn logs_follow_novad_live() {
    let novad = Novad::start("logs-follow");
    const BACKLOG: usize = 100;
    let mut child = novad.nova(&["logs", "-n", &BACKLOG.to_string()]).stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    // Within the whole backlog: novad logs a line per feature before it listens.
    let found = lines.by_ref().take(BACKLOG).map_while(Result::ok).any(|line| line.contains("listening on"));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(found, "the backlog should include novad's start");
}

/// With nothing on PATH, an enabled feature's binaries are missing: that
/// fails, while the running novad passes. (Quickshell may still be found:
/// the Nix package builds its launcher's path in.)
#[test]
fn doctor_fails_on_what_is_missing() {
    let novad = Novad::start("doctor");
    let config = novad.dir.join("config");
    std::fs::create_dir_all(config.join("nova")).unwrap();
    std::fs::write(config.join("nova/features.toml"), "enabled = [\"recording\"]\n").unwrap();
    let output = novad
        .nova(&["doctor"])
        // A checkout's layout, where the themes are found.
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("PATH", "")
        .env("XDG_CONFIG_HOME", &config)
        .env("NOVA_FEATURES_DIR", Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains(&format!("ok    novad       running {}", env!("CARGO_PKG_VERSION"))), "{text}");
    assert!(text.contains("FAIL  features    recording needs"), "{text}");
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("nova: "), "{output:?}");
}

#[test]
fn ipc_prints_novads_result_and_refuses_unknown_requests() {
    let novad = Novad::start("ipc");
    let status: Status = serde_json::from_str(&stdout(&novad.run(&["ipc", "status"]))).unwrap();
    assert_eq!(status.ui.state, UiState::Disabled);
    let output = novad.run(&["ipc", "focus_workspace", r#"{"id": "three"}"#]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no such request"), "{output:?}");
}

#[test]
fn completions_cover_the_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_nova")).args(["completions", "bash"]).output().unwrap();
    let script = stdout(&output);
    assert!(script.contains("doctor") && script.contains("--no-follow"), "{script}");
}

#[test]
fn toggle_reaches_the_ui_and_says_why_when_it_cant() {
    let novad = Novad::start("toggle");
    let output = novad.run(&["toggle", "ai-usage"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UI isn't connected"), "{output:?}");

    // Stands in for the UI: subscribes to toggle events.
    let ui = UnixStream::connect(novad.dir.join("novad.sock")).unwrap();
    ui.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    writeln!(&ui, r#"{{"v":2,"id":1,"cmd":"subscribe","args":{{"topics":["toggle"]}}}}"#).unwrap();
    assert!(lines.next().unwrap().unwrap().contains("hello"));
    assert!(lines.next().unwrap().unwrap().contains(r#""id":1"#));

    assert_eq!(stdout(&novad.run(&["toggle", "ai-usage"])), "", "quiet for keybinds");
    let event = lines.next().unwrap().unwrap();
    assert!(event.contains(r#""event":"toggle""#) && event.contains(r#""panel":"ai-usage""#), "{event}");

    let unknown = novad.run(&["toggle", "calculator"]);
    assert!(!unknown.status.success());
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(stderr.contains("ai-usage") && stderr.contains("launcher"), "lists the panels: {unknown:?}");
}

#[test]
fn volume_reaches_the_ui_quietly() {
    let novad = Novad::start("volume");
    let output = novad.run(&["volume", "up"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UI isn't connected"), "{output:?}");

    // Stands in for the UI: subscribes to volume events.
    let ui = UnixStream::connect(novad.dir.join("novad.sock")).unwrap();
    ui.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    writeln!(&ui, r#"{{"v":2,"id":1,"cmd":"subscribe","args":{{"topics":["volume"]}}}}"#).unwrap();
    assert!(lines.next().unwrap().unwrap().contains("hello"));
    assert!(lines.next().unwrap().unwrap().contains(r#""id":1"#));

    for key in ["up", "down", "mute"] {
        assert_eq!(stdout(&novad.run(&["volume", key])), "", "quiet for keybinds");
        let event = lines.next().unwrap().unwrap();
        assert_eq!(event, format!(r#"{{"v":2,"event":"volume","data":{{"key":"{key}"}}}}"#));
    }
    assert!(!novad.run(&["volume", "louder"]).status.success());
}

#[test]
fn lock_reaches_the_ui_quietly() {
    let novad = Novad::start("lock");
    let output = novad.run(&["lock"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UI isn't connected"), "{output:?}");

    // Stands in for the UI: subscribes to lock events, and gets the state.
    let ui = UnixStream::connect(novad.dir.join("novad.sock")).unwrap();
    ui.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    writeln!(&ui, r#"{{"v":2,"id":1,"cmd":"subscribe","args":{{"topics":["lock"]}}}}"#).unwrap();
    assert!(lines.next().unwrap().unwrap().contains("hello"));
    assert!(lines.next().unwrap().unwrap().contains(r#""id":1"#));
    assert!(lines.next().unwrap().unwrap().contains(r#""locked":false"#));

    // It answers once the UI says the screens are covered.
    std::thread::scope(|scope| {
        let locking = scope.spawn(|| novad.run(&["lock"]));
        let event = lines.next().unwrap().unwrap();
        assert!(event.contains(r#""event":"lock""#) && event.contains(r#""locked":true"#), "{event}");
        writeln!(&ui, r#"{{"v":2,"id":2,"cmd":"report_locked"}}"#).unwrap();
        assert_eq!(stdout(&locking.join().unwrap()), "", "quiet for keybinds");
    });
}

#[test]
fn status_without_novad_fails_with_a_hint() {
    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["status", "--socket", "/nonexistent/novad.sock"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Start it with `nova`"));
}

/// `nova daemon` as its own process, for tests that stop it.
struct DaemonProcess {
    dir: PathBuf,
    child: std::process::Child,
}

impl DaemonProcess {
    fn start(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nova-cli-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let features = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let child = Command::new(env!("CARGO_BIN_EXE_nova"))
            .arg("daemon")
            .arg("--no-ui")
            .arg("--socket")
            .arg(dir.join("novad.sock"))
            .arg("--log-dir")
            .arg(dir.join("logs"))
            .arg("--features-dir")
            .arg(features)
            .arg("--selection")
            .arg(dir.join("features.toml"))
            .arg("--config")
            .arg(dir.join("config.toml"))
            .arg("--state-dir")
            .arg(dir.join("state"))
            .arg("--codex-home")
            .arg(dir.join("codex"))
            .arg("--claude-credentials")
            .arg(dir.join("claude/.credentials.json"))
            .spawn()
            .unwrap();
        Self { dir, child }
    }

    fn socket(&self) -> PathBuf {
        self.dir.join("novad.sock")
    }

    fn connect(&self) -> UnixStream {
        let started = Instant::now();
        loop {
            if let Ok(stream) = UnixStream::connect(self.socket()) {
                return stream;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "the daemon never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for the daemon to exit on its own and checks it cleaned up.
    fn assert_exits(mut self) {
        assert!(self.child.wait().unwrap().success());
        assert!(!self.socket().exists());
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn nova_daemon_is_named_nova_and_quits_over_the_socket() {
    let daemon = DaemonProcess::start("daemon");
    let stream = daemon.connect();
    let mut writer = stream.try_clone().unwrap();
    let mut lines = BufReader::new(stream).lines();
    lines.next().unwrap().unwrap(); // hello
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", daemon.child.id())).unwrap();
    assert_eq!(comm.trim_end(), novad::PROCESS_NAME, "ps and btop show the daemon by this name");
    writeln!(writer, r#"{{"v":2,"id":1,"cmd":"quit"}}"#).unwrap();
    assert_eq!(lines.next().unwrap().unwrap(), r#"{"v":2,"id":1,"result":null}"#);
    daemon.assert_exits();
}

/// Even while the installed `nova.service` runs (as on a desktop running
/// Nova), `nova --socket <other> quit` stops the daemon on that socket.
#[test]
fn quit_stops_the_daemon_on_the_given_socket() {
    let daemon = DaemonProcess::start("quit");
    drop(daemon.connect());
    let output =
        Command::new(env!("CARGO_BIN_EXE_nova")).arg("quit").arg("--socket").arg(daemon.socket()).output().unwrap();
    assert_eq!(stdout(&output), "Nova stopped.\n");
    daemon.assert_exits();
}

#[test]
fn a_second_daemon_is_refused_before_touching_the_log() {
    let novad = Novad::start("second-daemon");
    let output = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["daemon", "--no-ui", "--log-dir", &novad.log_dir()])
        .arg("--socket")
        .arg(novad.dir.join("novad.sock"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    // The running daemon's log is still nova.log, not rotated to nova.log.1.
    let text = stdout(&novad.run(&["logs", "--no-follow", "--log-dir", &novad.log_dir()]));
    assert!(text.contains("listening on"), "{text}");
}

#[test]
fn claude_statusline_reports_limits_and_passes_input_on() {
    let novad = Novad::start_with("claude-statusline", "enabled = [\"ai-usage\"]\n");
    let input = r#"{"model":{"display_name":"Opus"},"rate_limits":{"five_hour":{"used_percentage":41.6,"resets_at":4102444800},"seven_day":{"used_percentage":61}}}"#;
    let mut child =
        novad.nova(&["claude-statusline", "--", "cat"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    assert_eq!(stdout(&child.wait_with_output().unwrap()), input, "the wrapped command gets the same input");

    let stream = UnixStream::connect(novad.dir.join("novad.sock")).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut writer = stream.try_clone().unwrap();
    writeln!(writer, r#"{{"v":2,"cmd":"subscribe","args":{{"topics":["ai_usage"]}}}}"#).unwrap();
    let event = BufReader::new(stream)
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(&line.unwrap()).unwrap())
        // The report isn't answered, so it may land just after subscribing.
        .find(|message| message["event"] == "ai_usage" && message["data"]["providers"][0]["state"] == "ready")
        .unwrap();
    let claude = &event["data"]["providers"][0];
    assert_eq!(
        claude["limits"][0],
        serde_json::json!({"window": "session", "percent": 42, "resets_at": "2100-01-01T00:00:00Z"})
    );
    assert_eq!(claude["limits"][1], serde_json::json!({"window": "weekly", "percent": 61}));
}

#[test]
fn claude_statusline_stays_quiet_without_novad() {
    let dir = std::env::temp_dir().join(format!("nova-cli-test-no-novad-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_nova"))
        .args(["claude-statusline", "--socket"])
        .arg(dir.join("missing.sock"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"rate_limits":{"seven_day":{"used_percentage":5}}}"#).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}
