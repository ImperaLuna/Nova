//! `nova status` and `nova version`.

use crate::client::Client;
use crate::lifecycle::RESTART_COMMAND;
use nova_protocol::{FeatureStatus, Status, UiState};
use std::path::Path;

const KIB_PER_MB: u64 = 1024;

pub fn show(socket: &Path, json: bool) -> Result<(), String> {
    let mut client = Client::connect(socket).map_err(|_| "Nova isn't running. Start it with `nova`.".to_owned())?;
    if json {
        println!("{}", client.request(nova_protocol::Command::Status)?);
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
    use nova_protocol::{Crash, FeatureState, UiStatus};

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
                FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] },
                FeatureState {
                    id: "recording".into(),
                    status: FeatureStatus::MissingDeps,
                    missing: vec!["wf-recorder".into()],
                },
            ],
            log_dir: "/x".into(),
        };
        let text = describe(&status);
        assert!(text.contains("daemon     running, pid 10, up 1m 5s, 10 MB"), "{text}");
        assert!(text.contains("UI         running, pid 11, 180 MB, restarted 2 times"), "{text}");
        assert!(text.contains("last crash not a time: see `nova logs --crash`"), "{text}");
        assert!(text.contains("features   core on, recording missing wf-recorder"), "{text}");
        assert!(!text.contains("warning"), "a supported Quickshell gets no row: {text}");

        let untested = Status { quickshell: Some("0.1.0".into()), ..status };
        let text = describe(&untested);
        assert!(text.contains("warning    Quickshell 0.1.0 is older than Nova is tested with"), "{text}");
    }
}
