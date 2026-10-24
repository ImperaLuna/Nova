//! Codex's limits from OpenAI's usage endpoint, the one Codex's `/status`
//! reads, signed in with Codex's own ChatGPT token. The logs only change
//! when Codex runs a turn, so without this their numbers can be days old.
//!
//! Off unless the user turns it on (`config`): OpenAI doesn't document this
//! use of Codex's sign-in. How the token is kept safe is in `endpoint`.

use super::codex::{self, Window};
use super::display_plan;
use super::endpoint::{self, Problem};
use jiff::Timestamp;
use nova_protocol::{UsageLimit, UsageWindow};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Codex's sign-in, in its home folder. Codex can keep it in the system
/// keyring instead; then there's no file and Nova can't ask.
pub fn auth_file(home: &Path) -> PathBuf {
    home.join("auth.json")
}

pub struct Token {
    access: String,
    /// The ChatGPT workspace the sign-in is for, sent along as Codex does.
    account: Option<String>,
}

/// The ChatGPT token in `path`, if it's there and private to the user. An
/// API key sign-in has no plan limits.
pub fn read_token(path: &Path) -> Result<Token, Problem> {
    let json = endpoint::read_sign_in(path)?;
    let tokens = json.get("tokens").ok_or(Problem::SignedOut)?;
    let text = |key: &str| tokens.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned);
    Ok(Token { access: text("access_token").ok_or(Problem::SignedOut)?, account: text("account_id") })
}

/// Asks `url` for the limits and the plan.
pub fn fetch(url: &str, token: &Token) -> Result<(Vec<UsageLimit>, Option<String>), Problem> {
    let authorization = format!("Bearer {}", token.access);
    let mut headers = vec![("Authorization", authorization.as_str())];
    if let Some(account) = &token.account {
        headers.push(("ChatGPT-Account-Id", account));
    }
    let json = endpoint::get_json(url, &headers, "OpenAI")?;
    let plan = json.get("plan_type").and_then(Value::as_str).and_then(display_plan);
    Ok((limits_from_usage(&json), plan))
}

/// The account's windows in the endpoint's answer; a model's own limits
/// (`additional_rate_limits`) aren't shown, as in the logs.
pub fn limits_from_usage(json: &Value) -> Vec<UsageLimit> {
    let Some(limit) = json.get("rate_limit") else { return vec![] };
    let windows = [("primary_window", UsageWindow::Session), ("secondary_window", UsageWindow::Weekly)]
        .into_iter()
        .filter_map(|(key, fallback)| {
            let window = limit.get(key).filter(|window| window.is_object())?;
            Some(Window {
                fallback,
                minutes: window.get("limit_window_seconds").and_then(Value::as_i64).map(|seconds| seconds / 60),
                used: window.get("used_percent").and_then(Value::as_f64)?,
                resets_at: window
                    .get("reset_at")
                    .and_then(Value::as_i64)
                    .and_then(|at| Timestamp::from_second(at).ok()),
            })
        });
    codex::usage_limits(windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(dir: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nova-codex-api-{dir}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = auth_file(&dir);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        path
    }

    #[test]
    fn reads_the_chatgpt_token_and_workspace() {
        let path = write(
            "ok",
            r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"i","access_token":"a","refresh_token":"r","account_id":"w"},"last_refresh":"2026-10-20T10:00:00Z"}"#,
        );
        let token = read_token(&path).unwrap();
        assert_eq!((token.access.as_str(), token.account.as_deref()), ("a", Some("w")));
    }

    #[test]
    fn an_api_key_sign_in_has_no_limits_to_ask_for() {
        let path = write("api-key", r#"{"OPENAI_API_KEY":"sk-test"}"#);
        assert_eq!(read_token(&path).err(), Some(Problem::SignedOut));
        let path = write("empty-token", r#"{"tokens":{"access_token":""}}"#);
        assert_eq!(read_token(&path).err(), Some(Problem::SignedOut));
    }

    /// The answer's shape, from Codex's backend client (codex-rs
    /// `RateLimitStatusPayload`), with a Plus plan's weekly-only limits.
    #[test]
    fn reads_the_accounts_windows_by_length() {
        let answer = json!({
            "plan_type": "plus",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {"used_percent": 6, "limit_window_seconds": 604800, "reset_after_seconds": 400000, "reset_at": 1793298916},
                "secondary_window": null
            },
            "credits": {"has_credits": false, "unlimited": false, "balance": "0"},
            "additional_rate_limits": [{"limit_name": "Other model", "metered_feature": "codex_other", "rate_limit": {
                "allowed": true, "limit_reached": false,
                "primary_window": {"used_percent": 90, "limit_window_seconds": 18000, "reset_after_seconds": 60, "reset_at": 1793000000}
            }}]
        });
        assert_eq!(
            limits_from_usage(&answer),
            [UsageLimit { window: UsageWindow::Weekly, percent: 6, resets_at: Some("2026-10-29T18:35:16Z".into()) }]
        );
    }

    #[test]
    fn a_session_and_a_weekly_window_go_session_first() {
        let answer = json!({"rate_limit": {
            "primary_window": {"used_percent": 12, "limit_window_seconds": 18000, "reset_at": 1793000000},
            "secondary_window": {"used_percent": 40, "limit_window_seconds": 604800, "reset_at": 1793298916}
        }});
        let windows: Vec<_> = limits_from_usage(&answer).iter().map(|limit| (limit.window, limit.percent)).collect();
        assert_eq!(windows, [(UsageWindow::Session, 12), (UsageWindow::Weekly, 40)]);
    }

    #[test]
    fn no_limits_in_the_answer_means_none() {
        assert_eq!(limits_from_usage(&json!({"plan_type": "free", "rate_limit": null})), []);
    }
}
