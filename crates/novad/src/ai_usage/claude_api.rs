//! Claude's limits from Anthropic's usage endpoint, the one Claude Code's
//! `/usage` screen reads, signed in with Claude Code's own OAuth token. It
//! works however Claude Code runs; the status line only runs in a terminal.
//!
//! Off unless the user turns it on (`config`): Anthropic doesn't document
//! this use of Claude Code's sign-in. How the token is kept safe is in
//! `endpoint`.

use super::endpoint::{self, Problem};
use super::{display_plan, round_percent};
use jiff::Timestamp;
use nova_protocol::{UsageLimit, UsageWindow};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";

/// Claude Code's credentials file: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn default_credentials() -> Option<PathBuf> {
    nova_paths::path_var("CLAUDE_CONFIG_DIR")
        .or_else(|| nova_paths::home().map(|home| home.join(".claude")))
        .map(|dir| dir.join(".credentials.json"))
}

pub struct Token {
    access: String,
    /// The subscription's display name, like `Max`.
    pub plan: Option<String>,
}

/// The token in `path`, if it's there, private to the user and hasn't
/// expired at `now`.
pub fn read_token(path: &Path, now: Timestamp) -> Result<Token, Problem> {
    let json = endpoint::read_sign_in(path)?;
    let oauth = json.get("claudeAiOauth").ok_or(Problem::SignedOut)?;
    let access = oauth.get("accessToken").and_then(Value::as_str).filter(|token| !token.is_empty());
    let access = access.ok_or(Problem::SignedOut)?.to_owned();
    if let Some(expires_ms) = oauth.get("expiresAt").and_then(Value::as_i64)
        && expires_ms <= now.as_millisecond()
    {
        return Err(Problem::Expired);
    }
    let plan = oauth.get("subscriptionType").and_then(Value::as_str).and_then(display_plan);
    Ok(Token { access, plan })
}

/// Asks `url` for the limits.
pub fn fetch(url: &str, token: &Token) -> Result<Vec<UsageLimit>, Problem> {
    let authorization = format!("Bearer {}", token.access);
    let headers = [("Authorization", authorization.as_str()), ("anthropic-beta", BETA_HEADER)];
    Ok(limits_from_usage(&endpoint::get_json(url, &headers, "Anthropic")?))
}

/// The windows in the endpoint's answer; others (per model, extra usage)
/// aren't shown.
pub fn limits_from_usage(json: &Value) -> Vec<UsageLimit> {
    [("five_hour", UsageWindow::Session), ("seven_day", UsageWindow::Weekly)]
        .into_iter()
        .filter_map(|(key, window)| {
            let limit = json.get(key)?;
            let used = limit.get("utilization")?.as_f64()?;
            let resets_at = limit.get("resets_at").and_then(Value::as_str).and_then(|at| at.parse::<Timestamp>().ok());
            Some(UsageLimit { window, percent: round_percent(used), resets_at: resets_at.map(|at| at.to_string()) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> Timestamp {
        "2026-10-23T10:00:00Z".parse().unwrap()
    }

    /// The file written and the folder it's in, which goes when dropped.
    fn write(dir: &str, text: &str) -> (nova_test_support::TempDir, PathBuf) {
        let dir = nova_test_support::TempDir::new(&format!("claude-api-{dir}"));
        let path = dir.join(".credentials.json");
        std::fs::write(&path, text).unwrap();
        set_mode(&path, 0o600);
        (dir, path)
    }

    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn reads_the_token_and_plan() {
        let (_dir, path) =
            write("ok", r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1792800000000,"subscriptionType":"max"}}"#);
        let token = read_token(&path, now()).unwrap();
        assert_eq!((token.access.as_str(), token.plan.as_deref()), ("t", Some("Max")));
    }

    #[test]
    fn an_expired_token_is_not_used() {
        let (_dir, path) = write("expired", r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1792700000000}}"#);
        assert_eq!(read_token(&path, now()).err(), Some(Problem::Expired));
    }

    #[test]
    fn no_file_or_no_claude_sign_in_means_signed_out() {
        assert_eq!(read_token(Path::new("/nonexistent/.credentials.json"), now()).err(), Some(Problem::SignedOut));
        let (_dir, path) = write("api-key", r#"{"other":{}}"#);
        assert_eq!(read_token(&path, now()).err(), Some(Problem::SignedOut));
    }

    #[test]
    fn reads_both_windows() {
        let answer = json!({
            "five_hour": {"utilization": 8.4, "resets_at": "2026-10-23T13:00:00.451561+00:00"},
            "seven_day": {"utilization": 17.0, "resets_at": "2026-10-29T21:00:00.000000+00:00"},
            "seven_day_opus": null,
            "extra_usage": {"is_enabled": false}
        });
        assert_eq!(
            limits_from_usage(&answer),
            [
                UsageLimit {
                    window: UsageWindow::Session,
                    percent: 8,
                    resets_at: Some("2026-10-23T13:00:00.451561Z".into())
                },
                UsageLimit { window: UsageWindow::Weekly, percent: 17, resets_at: Some("2026-10-29T21:00:00Z".into()) },
            ]
        );
    }

    #[test]
    fn missing_or_null_windows_are_left_out() {
        let answer = json!({"five_hour": null, "seven_day": {"utilization": 3, "resets_at": null}});
        assert_eq!(
            limits_from_usage(&answer),
            [UsageLimit { window: UsageWindow::Weekly, percent: 3, resets_at: None }]
        );
    }
}
