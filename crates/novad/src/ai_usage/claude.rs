//! Claude's limits, from the JSON Claude Code pipes into its status line
//! command (`rate_limits`, on Pro and Max plans). `nova claude-statusline`
//! is that command: it reads the JSON and reports only the limits to novad.

use super::round_percent;
use jiff::Timestamp;
use nova_protocol::{UsageLimit, UsageWindow};
use serde_json::Value;

/// The limits in a status line input, none when it has no `rate_limits`
/// (API key sign-in, or before Claude Code's first reply).
pub fn limits_from_statusline(input: &Value) -> Vec<UsageLimit> {
    let Some(limits) = input.get("rate_limits") else { return vec![] };
    let mut windows: Vec<UsageLimit> = [("five_hour", UsageWindow::Session), ("seven_day", UsageWindow::Weekly)]
        .into_iter()
        .filter_map(|(key, window)| {
            let limit = limits.get(key)?;
            let used = limit.get("used_percentage")?.as_f64()?;
            Some(UsageLimit {
                window,
                percent: round_percent(used),
                resets_at: limit.get("resets_at").and_then(reset_time).map(|at| at.to_string()),
            })
        })
        .collect();
    windows.sort_by_key(|limit| limit.window);
    windows
}

/// Unix seconds, or an RFC 3339 string.
fn reset_time(value: &Value) -> Option<Timestamp> {
    match value {
        Value::Number(number) => Timestamp::from_second(number.as_f64()? as i64).ok(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The status line input's shape, trimmed to what matters and with fixed
    /// times.
    fn input() -> Value {
        json!({
            "session_id": "abc",
            "model": {"id": "claude-opus-4-8", "display_name": "Opus"},
            "workspace": {"current_dir": "/home/me/project"},
            "rate_limits": {
                "five_hour": {"used_percentage": 41.6, "resets_at": 1792746000},
                "seven_day": {"used_percentage": 61, "resets_at": 1793095200}
            }
        })
    }

    #[test]
    fn reads_both_windows() {
        assert_eq!(
            limits_from_statusline(&input()),
            [
                UsageLimit {
                    window: UsageWindow::Session,
                    percent: 42,
                    resets_at: Some("2026-10-23T09:00:00Z".into())
                },
                UsageLimit { window: UsageWindow::Weekly, percent: 61, resets_at: Some("2026-10-27T10:00:00Z".into()) },
            ]
        );
    }

    #[test]
    fn no_rate_limits_means_nothing_to_report() {
        let mut input = input();
        input.as_object_mut().unwrap().remove("rate_limits");
        assert!(limits_from_statusline(&input).is_empty());
    }

    #[test]
    fn odd_values_are_tolerated() {
        let input = json!({"rate_limits": {
            "five_hour": {"used_percentage": 140.2, "resets_at": "2026-10-23T09:00:00Z"},
            "seven_day": {"used_percentage": "61"}
        }});
        let limits = limits_from_statusline(&input);
        assert_eq!(limits.len(), 1);
        assert_eq!((limits[0].percent, limits[0].resets_at.as_deref()), (100, Some("2026-10-23T09:00:00Z")));
    }
}
