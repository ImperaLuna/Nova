//! Codex's limits, from the session logs Codex writes itself
//! (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`). After each turn Codex
//! logs a `token_count` event holding the account's rate limits, so the
//! newest one is the current state. Nothing here talks to the network or
//! reads Codex's credentials.

use super::{display_plan, round_percent};
use jiff::Timestamp;
use nova_protocol::{UsageLimit, UsageWindow};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How much of a log's end is searched for its newest limits. A turn's
/// `token_count` is among its last lines.
const TAIL_BYTES: u64 = 512 * 1024;
/// Newest logs tried at startup before giving up on finding limits.
const LOGS_TRIED: usize = 20;
/// Windows up to a day long are the session limit, longer ones weekly.
const SESSION_WINDOW_MAX_MINUTES: i64 = 24 * 60;
/// The id of the account's own limits; other ids are a model's.
pub const MAIN_LIMIT_ID: &str = "codex";

#[derive(Debug, Clone, PartialEq)]
pub struct CodexUsage {
    pub plan: Option<String>,
    pub limits: Vec<UsageLimit>,
    /// When Codex logged them; a newer entry replaces an older one.
    pub logged_at: Timestamp,
}

/// `$CODEX_HOME`, else `~/.codex`.
pub fn default_home() -> Option<PathBuf> {
    nova_paths::dir_var("CODEX_HOME").or_else(|| nova_paths::home().map(|home| home.join(".codex")))
}

pub fn sessions_dir(home: &Path) -> PathBuf {
    home.join("sessions")
}

/// The newest limits in any session log. The most recently written log
/// needn't hold them: with two sessions open, one can log other lines after
/// the other logged newer limits. So logs are read newest first until one
/// was last written before the newest limits found so far.
pub fn newest(sessions: &Path) -> Option<CodexUsage> {
    let mut logs = Vec::new();
    collect_logs(sessions, &mut logs);
    // Newest first. File times are coarse, so ties are common; Codex's file
    // names start with the session's time, so the later name is newer.
    logs.sort_by(|a, b| b.cmp(a));
    let mut best: Option<CodexUsage> = None;
    for (modified, path) in logs.into_iter().take(LOGS_TRIED) {
        if let Some(best) = &best
            && Timestamp::try_from(modified).is_ok_and(|modified| modified < best.logged_at)
        {
            break;
        }
        if let Some(found) = read_log(&path)
            && best.as_ref().is_none_or(|best| found.logged_at > best.logged_at)
        {
            best = Some(found);
        }
    }
    best
}

fn collect_logs(dir: &Path, logs: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect_logs(&path, logs),
            Ok(kind) if kind.is_file() && is_log(&path) => {
                let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                logs.push((modified, path));
            }
            _ => {}
        }
    }
}

pub fn is_log(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "jsonl")
}

/// The newest limits in one log, from the end of the file.
pub fn read_log(path: &Path) -> Option<CodexUsage> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    // A tail that starts mid-file starts mid-line; parsing skips that line.
    text.lines().rev().filter(|line| line.contains("\"rate_limits\"")).find_map(parse_line)
}

/// One log line, when it's a `token_count` event with Codex's own limits.
/// Reads both the current shape (`window_minutes`, `resets_at` in Unix
/// seconds) and the older one (`resets_in_seconds` from the line's time).
pub fn parse_line(line: &str) -> Option<CodexUsage> {
    let entry: Value = serde_json::from_str(line).ok()?;
    let payload = entry.get("payload")?;
    if payload.get("type")?.as_str()? != "token_count" {
        return None;
    }
    let limits = payload.get("rate_limits")?.as_object()?;
    // Models with limits of their own log them under another id.
    if limits.get("limit_id").and_then(Value::as_str).is_some_and(|id| id != MAIN_LIMIT_ID) {
        return None;
    }
    let logged_at: Timestamp = entry.get("timestamp")?.as_str()?.parse().ok()?;
    let windows = [("primary", UsageWindow::Session), ("secondary", UsageWindow::Weekly)].into_iter().filter_map(
        |(key, fallback)| {
            let window = limits.get(key).filter(|window| window.is_object())?;
            let resets_at = match window.get("resets_at").and_then(Value::as_i64) {
                Some(seconds) => Timestamp::from_second(seconds).ok(),
                None => window
                    .get("resets_in_seconds")
                    .and_then(Value::as_i64)
                    .and_then(|seconds| logged_at.checked_add(jiff::SignedDuration::from_secs(seconds)).ok()),
            };
            Some(Window {
                fallback,
                minutes: window.get("window_minutes").and_then(Value::as_i64),
                used: window.get("used_percent").and_then(Value::as_f64)?,
                resets_at,
            })
        },
    );
    let windows = usage_limits(windows);
    if windows.is_empty() {
        return None;
    }
    let plan = limits.get("plan_type").and_then(Value::as_str).and_then(display_plan);
    Some(CodexUsage { plan, limits: windows, logged_at })
}

/// One of Codex's limit windows, as its logs and its usage endpoint give it.
pub struct Window {
    /// What it is when its length isn't given: the first is the session.
    pub fallback: UsageWindow,
    pub minutes: Option<i64>,
    pub used: f64,
    pub resets_at: Option<Timestamp>,
}

/// Codex's windows as limits, session first. Which window is which goes by
/// its length: plans differ in which they have and in what order.
pub fn usage_limits(windows: impl IntoIterator<Item = Window>) -> Vec<UsageLimit> {
    let mut limits: Vec<UsageLimit> = Vec::new();
    for window in windows {
        let kind = match window.minutes {
            Some(minutes) if minutes <= SESSION_WINDOW_MAX_MINUTES => UsageWindow::Session,
            Some(_) => UsageWindow::Weekly,
            None => window.fallback,
        };
        if limits.iter().all(|limit| limit.window != kind) {
            limits.push(UsageLimit {
                window: kind,
                percent: round_percent(window.used),
                resets_at: window.resets_at.map(|at| at.to_string()),
            });
        }
    }
    limits.sort_by_key(|limit| limit.window);
    limits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shapes recorded from Codex logs, with the times swapped for fixed ones.
    const PLUS: &str = r#"{"timestamp":"2026-10-23T06:40:12.345Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":12000,"cached_input_tokens":9000,"output_tokens":800,"reasoning_output_tokens":300,"total_tokens":12800},"model_context_window":272000},"rate_limits":{"primary":{"used_percent":12.0,"window_minutes":300,"resets_at":1792746000},"secondary":{"used_percent":84.4,"window_minutes":10080,"resets_at":1793095200},"plan_type":"plus"}}}"#;
    const PRO_WEEKLY_ONLY: &str = r#"{"timestamp":"2026-10-23T06:40:12.345Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":30.5,"window_minutes":10080,"resets_at":1793095200},"secondary":null,"plan_type":"pro"}}}"#;
    /// codex-cli 0.163: Plus with only a weekly window, and more keys.
    const PLUS_WEEKLY_ONLY: &str = r#"{"timestamp":"2026-10-23T16:58:25.483Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":8.0,"window_minutes":10080,"resets_at":1793298916},"secondary":null,"credits":{"has_credits":false,"unlimited":false,"balance":"0"},"individual_limit":null,"spend_control_reached":null,"plan_type":"plus","rate_limit_reached_type":null}}}"#;
    const OLD_SHAPE: &str = r#"{"timestamp":"2026-10-23T06:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":5.0,"window_minutes":299,"resets_in_seconds":600},"secondary":{"used_percent":40.0,"window_minutes":10079,"resets_in_seconds":3600}}}}"#;

    #[test]
    fn plus_has_a_session_and_a_weekly_limit() {
        let usage = parse_line(PLUS).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Plus"));
        assert_eq!(
            usage.limits,
            [
                UsageLimit {
                    window: UsageWindow::Session,
                    percent: 12,
                    resets_at: Some("2026-10-23T09:00:00Z".into())
                },
                UsageLimit { window: UsageWindow::Weekly, percent: 84, resets_at: Some("2026-10-27T10:00:00Z".into()) },
            ]
        );
    }

    #[test]
    fn a_weekly_only_plan_reports_just_that() {
        let usage = parse_line(PRO_WEEKLY_ONLY).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Pro"));
        assert_eq!(usage.limits.len(), 1);
        assert_eq!((usage.limits[0].window, usage.limits[0].percent), (UsageWindow::Weekly, 31));
    }

    #[test]
    fn plus_may_have_only_a_weekly_limit() {
        let usage = parse_line(PLUS_WEEKLY_ONLY).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("Plus"));
        assert_eq!(
            usage.limits,
            [UsageLimit { window: UsageWindow::Weekly, percent: 8, resets_at: Some("2026-10-29T18:35:16Z".into()) }]
        );
    }

    /// A model with limits of its own logs them under its id; they aren't
    /// the account's.
    #[test]
    fn a_models_own_limits_are_skipped() {
        let model = PLUS_WEEKLY_ONLY.replace(r#""limit_id":"codex""#, r#""limit_id":"codex_other_model""#);
        assert_eq!(parse_line(&model), None);
    }

    #[test]
    fn older_logs_count_resets_from_the_line_time() {
        let usage = parse_line(OLD_SHAPE).unwrap();
        assert_eq!(usage.plan, None);
        assert_eq!(usage.limits[0].resets_at.as_deref(), Some("2026-10-23T06:10:00Z"));
        assert_eq!(usage.limits[1].resets_at.as_deref(), Some("2026-10-23T07:00:00Z"));
    }

    /// File times advance only every few milliseconds, so logs written close
    /// together often share one; then the name, which starts with Codex's
    /// timestamp, decides. Several pairs, since the directory order varies.
    #[test]
    fn logs_with_the_same_time_go_by_name() {
        let root = nova_test_support::TempDir::new("codex-same-time");
        let time = SystemTime::now() - std::time::Duration::from_secs(60);
        for pair in 0..10 {
            let dir = root.join(pair.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            for (hour, session) in [("06", 1), ("07", 12)] {
                let log = dir.join(format!("rollout-2026-10-23T{hour}-00-00-{pair}.jsonl"));
                std::fs::write(&log, PLUS.replace("\"used_percent\":12.0", &format!("\"used_percent\":{session}.0")))
                    .unwrap();
                std::fs::File::options().write(true).open(&log).unwrap().set_modified(time).unwrap();
            }
            assert_eq!(newest(&dir).unwrap().limits[0].percent, 12, "pair {pair}");
        }
    }

    /// Session A logged limits, then session B newer ones, then A wrote
    /// another line: A is the newest file, B has the newest limits.
    #[test]
    fn the_newest_limits_win_over_the_newest_file() {
        let dir = nova_test_support::TempDir::new("codex-two-sessions");
        let now = SystemTime::now();
        let older = PLUS.replace("06:40:12", "06:30:00").replace("\"used_percent\":12.0", "\"used_percent\":5.0");
        let a = dir.join("rollout-2026-10-23T06-00-00-a.jsonl");
        std::fs::write(
            &a,
            older + "\n" + r#"{"timestamp":"2026-10-23T06:50:00Z","type":"response_item","payload":{}}"#,
        )
        .unwrap();
        let b = dir.join("rollout-2026-10-23T06-10-00-b.jsonl");
        std::fs::write(&b, PLUS).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(now - std::time::Duration::from_secs(60))
            .unwrap();
        std::fs::File::options().write(true).open(&a).unwrap().set_modified(now).unwrap();
        assert_eq!(newest(&dir).unwrap().limits[0].percent, 12);
    }

    #[test]
    fn other_lines_are_skipped() {
        assert_eq!(
            parse_line(
                r#"{"timestamp":"2026-10-23T06:00:00Z","type":"event_msg","payload":{"type":"agent_message","message":"rate_limits"}}"#
            ),
            None
        );
        assert_eq!(
            parse_line(
                r#"{"timestamp":"2026-10-23T06:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#
            ),
            None
        );
        assert_eq!(parse_line("{\"rate_limits\": truncated"), None);
    }
}
