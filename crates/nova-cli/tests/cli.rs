//! Integration tests: the real `nova` binary against a real novad (in this
//! process, without the UI) over a real socket.

use nova_protocol::{Status, UiState};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Novad {
    dir: PathBuf,
}

impl Novad {
    fn start(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nova-cli-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let features = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let args: Vec<std::ffi::OsString> = vec![
            "novad".into(),
            "--no-ui".into(),
            "--socket".into(),
            dir.join("novad.sock").into(),
            "--log-dir".into(),
            dir.join("logs").into(),
            "--features-dir".into(),
            features.into(),
            "--selection".into(),
            dir.join("features.toml").into(),
        ];
        novad::spawn(<novad::Args as clap::Parser>::parse_from(args)).unwrap();
        Self { dir }
    }

    fn nova(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nova"));
        command.args(args).arg("--socket").arg(self.dir.join("novad.sock"));
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.nova(args).output().unwrap()
    }

    fn log_dir(&self) -> String {
        self.dir.join("logs").display().to_string()
    }
}

impl Drop for Novad {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
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
    assert!(text.contains("novad      running"), "{text}");
    assert!(text.contains("features   core on"), "{text}");
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
    let mut child = novad.nova(&["logs", "-n", "100"]).stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let found = lines.by_ref().take(20).map_while(Result::ok).any(|line| line.contains("listening on"));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(found, "the backlog should include novad's start");
}

#[test]
fn planned_commands_say_so_and_fail() {
    let novad = Novad::start("planned");
    let output = novad.run(&["setup"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("`nova setup` isn't built yet"));
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
