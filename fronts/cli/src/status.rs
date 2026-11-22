//! `nova status` and `nova version`.

use crate::client::{Client, RESTART_COMMAND};
use nova_daemon_info::{FeatureStatus, Status, UiState};
use nova_desktop_info::Desktop;
use std::path::Path;

const KIB_PER_MB: u64 = 1024;

pub fn show(socket: &Path, json: bool) -> Result<(), String> {
    let mut client = Client::reach(socket).map_err(|unreachable| unreachable.advice())?;
    if json {
        println!("{}", client.request(nova_daemon_info::Command::Status)?);
    } else {
        print!("{}", describe(&client.status()?));
    }
    Ok(())
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

fn describe(status: &Status) -> String {
    let mut out = String::new();
    let mut daemon = format!("running, pid {}, up {}", status.pid, duration(status.uptime_secs));
    if let Some(kib) = status.pss_kib {
        daemon += &format!(", {}", megabytes(kib));
    }
    out += &row("daemon", &daemon);

    let ui = &status.ui;
    let mut line = match (ui.state, ui.pid) {
        (UiState::Running, Some(pid)) => format!("running, pid {pid}"),
        (UiState::Running, None) => "running".into(),
        (UiState::Restarting, _) => "stopped, starting again".into(),
        (UiState::Disabled, _) => "not run by the daemon (--no-ui)".into(),
        (UiState::NotNeeded, _) => "not running: no feature with a UI is on".into(),
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
            FeatureStatus::Ready => match &f.mode {
                Some(mode) => format!("{} on ({mode})", f.id),
                None => format!("{} on", f.id),
            },
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
    use nova_daemon_info::{Crash, FeatureState, UiStatus};
    use nova_desktop_info::{FocusedWindow, Output, Workspace};

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
                    layout: 0,
                },
                Output {
                    name: "HDMI-A-1".into(),
                    focused: false,
                    workspaces: vec![workspace(4, "1", true)],
                    layout: 0,
                },
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
    fn durations_read_naturally() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(310), "5m 10s");
        assert_eq!(duration(7380), "2h 3m");
    }

    #[test]
    fn status_mentions_restarts_crashes_and_missing_packages() {
        let status = Status {
            daemon: "0.1.0".into(),
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
                FeatureState {
                    id: "bar".into(),
                    status: FeatureStatus::Ready,
                    missing: vec![],
                    slots: vec![],
                    mode: None,
                },
                FeatureState {
                    id: "nova-files".into(),
                    status: FeatureStatus::Ready,
                    missing: vec![],
                    slots: vec![],
                    mode: Some("full".into()),
                },
                FeatureState {
                    id: "recording".into(),
                    status: FeatureStatus::MissingDeps,
                    missing: vec!["wf-recorder".into()],
                    slots: vec![],
                    mode: None,
                },
            ],
            log_dir: "/x".into(),
            desktop: None,
        };
        let text = describe(&status);
        assert!(text.contains("daemon     running, pid 10, up 1m 5s, 10 MB"), "{text}");
        assert!(text.contains("UI         running, pid 11, 180 MB, restarted 2 times"), "{text}");
        assert!(text.contains("last crash not a time: see `nova logs --crash`"), "{text}");
        assert!(text.contains("features   bar on, nova-files on (full), recording missing wf-recorder"), "{text}");
        assert!(!text.contains("warning"), "a supported Quickshell gets no row: {text}");
        assert!(text.contains("compositor none followed"), "{text}");

        let untested = Status { quickshell: Some("0.1.0".into()), ..status };
        let text = describe(&untested);
        assert!(text.contains("warning    Quickshell 0.1.0 is older than Nova is tested with"), "{text}");
    }
}
