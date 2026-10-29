//! `nova status` and `nova version`.

use crate::client::Client;
use crate::lifecycle::RESTART_COMMAND;
use nova_protocol::{Desktop, FeatureStatus, Status, UiState};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const KIB_PER_MB: u64 = 1024;
/// How long `nova status` waits for GitHub's answer.
const GITHUB_TIMEOUT: Duration = Duration::from_secs(5);
/// How many characters of a commit hash `nova status` shows.
const SHORT_REVISION: usize = 7;
/// The end of a revision built from a checkout with uncommitted changes.
const DIRTY: &str = "-dirty";

pub fn show(socket: &Path, json: bool) -> Result<(), String> {
    let mut client = Client::reach(socket).map_err(|unreachable| unreachable.advice())?;
    if json {
        println!("{}", client.request(nova_protocol::Command::Status)?);
    } else {
        let status = client.status()?;
        let build = status.revision.as_deref().map(Build::parse);
        let main = build.as_ref().and_then(|build| against_main(build.commit));
        print!("{}", describe(&status, main));
    }
    Ok(())
}

/// The commit novad was built from, as its package records it.
struct Build<'a> {
    commit: &'a str,
    /// Built from a checkout with uncommitted changes.
    dirty: bool,
}

impl<'a> Build<'a> {
    fn parse(revision: &'a str) -> Self {
        match revision.strip_suffix(DIRTY) {
            Some(commit) => Build { commit, dirty: true },
            None => Build { commit: revision, dirty: false },
        }
    }
}

/// Where a build stands against main.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AgainstMain {
    /// main has these many commits the build doesn't (0: it's main).
    Behind(u64),
    /// The build has commits main doesn't (a branch), and main has these
    /// many it doesn't.
    OffMain(u64),
}

/// Where `commit` stands against main, asked of GitHub with the `gh` CLI
/// (the repository is private). `None` without gh, its login or the network.
fn against_main(commit: &str) -> Option<AgainstMain> {
    // From the socket, so checked before it goes in a URL.
    if commit.is_empty() || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut gh = Command::new("gh");
    let endpoint = format!("repos/{}/compare/{commit}...main", github_repo!());
    gh.args(["api", &endpoint, "--jq", r#""\(.behind_by) \(.ahead_by)""#]);
    let output = novad::output_within(gh, GITHUB_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    // behind_by: commits the build has that main doesn't; ahead_by: main's.
    let text = String::from_utf8(output.stdout).ok()?;
    let (extra, newer) = text.trim().split_once(' ')?;
    let (extra, newer): (u64, u64) = (extra.parse().ok()?, newer.parse().ok()?);
    Some(if extra == 0 { AgainstMain::Behind(newer) } else { AgainstMain::OffMain(newer) })
}

/// The commit novad runs, short, and where it stands against main.
fn describe_build(build: &Build, main: Option<AgainstMain>) -> String {
    let short: String = build.commit.chars().take(SHORT_REVISION).collect();
    let dirty = if build.dirty { " with uncommitted changes" } else { "" };
    let commits = |n: u64| if n == 1 { "1 commit".to_owned() } else { format!("{n} commits") };
    let main = match main {
        Some(AgainstMain::Behind(0)) if !build.dirty => ", current with main".to_owned(),
        Some(AgainstMain::Behind(0)) => ", on main".to_owned(),
        Some(AgainstMain::Behind(n)) => format!(", {} behind main", commits(n)),
        Some(AgainstMain::OffMain(0)) => ", not on main".to_owned(),
        Some(AgainstMain::OffMain(n)) => format!(", not on main, which has {} newer", commits(n)),
        None => String::new(),
    };
    row("build", &format!("{}{dirty}{main}", printable(&short)))
}

/// Prints nova's version. A running novad from another version is pointed
/// out, since it keeps running the old code until it's restarted.
pub fn version(socket: &Path) {
    let version = env!("CARGO_PKG_VERSION");
    println!("nova {version}");
    if let Ok(client) = Client::connect(socket)
        && client.daemon_version != version
    {
        println!("The running daemon is {}. Run `{RESTART_COMMAND}` to switch to {version}.", client.daemon_version);
    }
}

/// `main`: where the running build stands against main, when known.
fn describe(status: &Status, main: Option<AgainstMain>) -> String {
    let mut out = String::new();
    let mut daemon = format!("running, pid {}, up {}", status.pid, duration(status.uptime_secs));
    if let Some(kib) = status.pss_kib {
        daemon += &format!(", {}", megabytes(kib));
    }
    out += &row("daemon", &daemon);
    if let Some(revision) = &status.revision {
        out += &describe_build(&Build::parse(revision), main);
    }

    let ui = &status.ui;
    let mut line = match (ui.state, ui.pid) {
        (UiState::Running, Some(pid)) => format!("running, pid {pid}"),
        (UiState::Running, None) => "running".into(),
        (UiState::Restarting, _) => "stopped, starting again".into(),
        (UiState::Disabled, _) => "not run by the daemon (--no-ui)".into(),
    };
    if let Some(kib) = ui.pss_kib {
        line += &format!(", {}", megabytes(kib));
    }
    if ui.restarts > 0 {
        line += &format!(", restarted {} time{}", ui.restarts, if ui.restarts == 1 { "" } else { "s" });
    }
    out += &row("UI", &line);
    if let Some(crash) = &ui.last_crash {
        out += &row("", &format!("last crash {}: see `nova logs --crash`", ago(&crash.time)));
    }

    // Only when it's a problem: a supported Quickshell isn't worth a row.
    if let Some(problem) =
        status.quickshell.as_deref().and_then(|version| nova_quickshell::Range::supported().problem(version))
    {
        out += &row("warning", &problem);
    }

    out += &describe_desktop(status.desktop.as_ref());

    let features: Vec<String> = status
        .features
        .iter()
        .map(|f| match f.status {
            FeatureStatus::Ready => format!("{} on", f.id),
            FeatureStatus::Off => format!("{} off", f.id),
            FeatureStatus::MissingDeps => format!("{} missing {}", f.id, f.missing.join(", ")),
        })
        .collect();
    out += &row("features", &features.join(", "));
    out
}

/// The compositor, its workspaces per output and the focused window.
fn describe_desktop(desktop: Option<&Desktop>) -> String {
    let Some(desktop) = desktop else {
        return row("compositor", "none followed (Nova knows niri and Hyprland)");
    };
    let mut compositor = desktop.compositor.clone();
    if let Some(layout) = &desktop.keyboard_layout {
        compositor += &format!(", keyboard {}", printable(layout));
    }
    let mut out = row("compositor", &compositor);
    for (index, output) in desktop.outputs.iter().enumerate() {
        let workspaces: Vec<String> = output
            .workspaces
            .iter()
            .map(|workspace| {
                let marks: Vec<&str> = [
                    (workspace.special, "special"),
                    (workspace.active, "shown"),
                    (workspace.urgent, "urgent"),
                    (workspace.empty, "empty"),
                ]
                .into_iter()
                .filter_map(|(set, mark)| set.then_some(mark))
                .collect();
                if marks.is_empty() {
                    printable(&workspace.name)
                } else {
                    format!("{} ({})", printable(&workspace.name), marks.join(", "))
                }
            })
            .collect();
        let focused = if output.focused { " (focused)" } else { "" };
        let label = if index == 0 { "workspaces" } else { "" };
        out += &row(label, &format!("{}{focused}: {}", printable(&output.name), workspaces.join(", ")));
    }
    let window = match &desktop.focused_window {
        Some(window) if window.app_id.is_empty() => printable(&window.title),
        Some(window) => format!("{}: {}", printable(&window.app_id), printable(&window.title)),
        None => "none focused".into(),
    };
    out + &row("window", &window)
}

/// `text` with control characters escaped (`\u{1b}`), so text from apps,
/// like a web page's title, can't send the terminal escape sequences.
pub fn printable(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { c.escape_unicode().to_string() } else { c.to_string() }).collect()
}

fn row(label: &str, value: &str) -> String {
    format!("{label:<11}{value}\n")
}

fn megabytes(kib: u64) -> String {
    format!("{} MB", kib / KIB_PER_MB)
}

/// `2h 3m`, `5m 10s`, `42s`.
fn duration(secs: u64) -> String {
    let (hours, minutes, seconds) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (hours, minutes) {
        (0, 0) => format!("{seconds}s"),
        (0, _) => format!("{minutes}m {seconds}s"),
        _ => format!("{hours}h {minutes}m"),
    }
}

/// `3m 5s ago` from an RFC 3339 time, or the time itself if it won't parse.
fn ago(time: &str) -> String {
    match time.parse::<jiff::Timestamp>() {
        Ok(then) => {
            let secs = jiff::Timestamp::now().duration_since(then).as_secs().max(0);
            format!("{} ago", duration(secs.unsigned_abs()))
        }
        Err(_) => time.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::{Crash, FeatureState, FocusedWindow, Output, UiStatus, Workspace};

    #[test]
    fn the_desktop_lists_workspaces_per_output() {
        let workspace = |id, name: &str, active| Workspace {
            id,
            name: name.into(),
            active,
            focused: false,
            urgent: false,
            empty: false,
            fullscreen: false,
            special: false,
            shown_elsewhere: false,
            active_window: None,
        };
        let desktop = Desktop {
            compositor: "niri".into(),
            capabilities: vec![],
            outputs: vec![
                Output {
                    name: "DP-1".into(),
                    focused: true,
                    workspaces: vec![
                        Workspace { empty: true, ..workspace(1, "1", false) },
                        Workspace { focused: true, ..workspace(2, "web", true) },
                        Workspace { urgent: true, ..workspace(3, "3", false) },
                    ],
                },
                Output { name: "HDMI-A-1".into(), focused: false, workspaces: vec![workspace(4, "1", true)] },
            ],
            focused_window: Some(FocusedWindow { id: "1".into(), app_id: "foot".into(), title: "~/Projects".into() }),
            keyboard_layout: Some("English (US)".into()),
        };
        assert_eq!(
            describe_desktop(Some(&desktop)),
            "compositor niri, keyboard English (US)\n\
             workspaces DP-1 (focused): 1 (empty), web (shown), 3 (urgent)\n\
             \x20          HDMI-A-1: 1 (shown)\n\
             window     foot: ~/Projects\n"
        );
    }

    #[test]
    fn window_titles_cant_send_terminal_escapes() {
        let desktop = Desktop {
            compositor: "niri".into(),
            capabilities: vec![],
            outputs: vec![],
            focused_window: Some(FocusedWindow {
                id: "1".into(),
                app_id: "firefox".into(),
                title: "page\x1b]52;c;cHdu\x07".into(),
            }),
            keyboard_layout: None,
        };
        let text = describe_desktop(Some(&desktop));
        assert!(!text.contains('\x1b') && !text.contains('\x07'), "{text:?}");
        assert!(text.contains("page\\u{1b}]52;c;cHdu\\u{7}"), "{text:?}");
    }

    #[test]
    fn the_build_row_names_the_commit_and_where_it_stands_against_main() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let row = |revision: &str, main| describe_build(&Build::parse(revision), main);
        assert_eq!(row(commit, Some(AgainstMain::Behind(0))), "build      0123456, current with main\n");
        assert_eq!(row(commit, Some(AgainstMain::Behind(1))), "build      0123456, 1 commit behind main\n");
        assert_eq!(row(commit, Some(AgainstMain::Behind(12))), "build      0123456, 12 commits behind main\n");
        assert_eq!(row(commit, Some(AgainstMain::OffMain(0))), "build      0123456, not on main\n");
        assert_eq!(
            row(commit, Some(AgainstMain::OffMain(2))),
            "build      0123456, not on main, which has 2 commits newer\n"
        );
        assert_eq!(row(commit, None), "build      0123456\n");
        let dirty = format!("{commit}-dirty");
        assert_eq!(row(&dirty, Some(AgainstMain::Behind(0))), "build      0123456 with uncommitted changes, on main\n");
        assert_eq!(
            row(&dirty, Some(AgainstMain::Behind(3))),
            "build      0123456 with uncommitted changes, 3 commits behind main\n"
        );
    }

    #[test]
    fn only_a_hex_commit_is_asked_about() {
        assert_eq!(against_main("x/../../user?"), None);
        assert_eq!(against_main(""), None);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(310), "5m 10s");
        assert_eq!(duration(7380), "2h 3m");
    }

    #[test]
    fn status_mentions_restarts_crashes_and_missing_packages() {
        let status = Status {
            daemon: "0.1.0".into(),
            revision: None,
            pid: 10,
            uptime_secs: 65,
            pss_kib: Some(10 * 1024),
            quickshell: Some("0.3.2".into()),
            ui: UiStatus {
                state: UiState::Running,
                pid: Some(11),
                pss_kib: Some(180 * 1024),
                restarts: 2,
                last_crash: Some(Crash { time: "not a time".into(), log: "/x.log".into() }),
            },
            features: vec![
                FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] },
                FeatureState {
                    id: "recording".into(),
                    status: FeatureStatus::MissingDeps,
                    missing: vec!["wf-recorder".into()],
                },
            ],
            log_dir: "/x".into(),
            desktop: None,
        };
        let text = describe(&status, None);
        assert!(text.contains("daemon     running, pid 10, up 1m 5s, 10 MB"), "{text}");
        assert!(text.contains("UI         running, pid 11, 180 MB, restarted 2 times"), "{text}");
        assert!(text.contains("last crash not a time: see `nova logs --crash`"), "{text}");
        assert!(text.contains("features   core on, recording missing wf-recorder"), "{text}");
        assert!(!text.contains("warning"), "a supported Quickshell gets no row: {text}");
        assert!(text.contains("compositor none followed"), "{text}");
        assert!(!text.contains("build"), "a checkout build gets no row: {text}");

        let untested = Status { quickshell: Some("0.1.0".into()), ..status };
        let text = describe(&untested, None);
        assert!(text.contains("warning    Quickshell 0.1.0 is older than Nova is tested with"), "{text}");
    }
}
