//! Claude's limits from Anthropic's usage endpoint, the one Claude Code's
//! `/usage` screen reads, signed in with Claude Code's own OAuth token. It
//! works however Claude Code runs; the status line only runs in a terminal.
//!
//! Off unless the user turns it on (`config`): Anthropic doesn't document
//! this use of Claude Code's sign-in.
//!
//! The token is read from Claude Code's credentials file before each request
//! and goes nowhere but that request's header, sent to Anthropic only (no
//! redirects) under Nova's own user agent. A credentials file other users
//! could read, or that isn't the user's, isn't used. It is never refreshed: that
//! would rotate Claude Code's sign-in under it. When it has expired, Claude
//! Code renews it the next time it runs.

use super::round_percent;
use jiff::Timestamp;
use nova_protocol::{UsageLimit, UsageWindow};
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";
const TIMEOUT: Duration = Duration::from_secs(10);
/// The answer is well under 1 KiB.
const MAX_BODY_BYTES: u64 = 64 * 1024;
const USER_AGENT: &str = concat!("nova/", env!("CARGO_PKG_VERSION"));
/// Permission bits for anyone but the owner.
const GROUP_OTHER_BITS: u32 = 0o077;

/// Claude Code's credentials file: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn default_credentials() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .map(|dir| dir.join(".credentials.json"))
}

pub struct Token {
    access: String,
    /// The subscription's display name, like `Max`.
    pub plan: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Problem {
    /// No credentials file, or no Claude.ai sign-in in it.
    SignedOut,
    /// The token has expired, or Anthropic refused it.
    Expired,
    /// The credentials file isn't the user's, or others can read it.
    Unsafe(PathBuf),
    /// Anthropic asked to slow down, for this long if it said.
    RateLimited(Option<Duration>),
    Failed(String),
}

impl Problem {
    /// What the panel says when there are no numbers.
    pub fn hint(&self) -> String {
        match self {
            Problem::SignedOut => "Sign in to Claude Code with your Claude account to see your limits.".into(),
            Problem::Expired => "Claude Code's sign-in has expired. Open Claude Code to renew it.".into(),
            Problem::Unsafe(path) => format!(
                "Nova won't use {} because other users could read it or it isn't yours. To fix it, run\nchmod 600 {}",
                path.display(),
                path.display()
            ),
            Problem::RateLimited(_) => "Anthropic is busy; trying again in a while.".into(),
            Problem::Failed(e) => format!("Can't get Claude's limits: {e}"),
        }
    }
}

/// The token in `path`, if it's there, private to the user and hasn't
/// expired at `now`.
pub fn read_token(path: &Path, now: Timestamp) -> Result<Token, Problem> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Problem::SignedOut),
        Err(e) => return Err(Problem::Failed(format!("can't read {}: {e}", path.display()))),
    };
    token_from(file, path, now)
}

/// The token in `file`, opened from `path`. The checks look at the open
/// file, so a file swapped in at `path` after them is never read.
fn token_from(mut file: File, path: &Path, now: Timestamp) -> Result<Token, Problem> {
    let unreadable = |e: std::io::Error| Problem::Failed(format!("can't read {}: {e}", path.display()));
    let metadata = file.metadata().map_err(unreadable)?;
    // SAFETY: geteuid has no preconditions and can't fail.
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & GROUP_OTHER_BITS != 0 {
        return Err(Problem::Unsafe(path.to_owned()));
    }
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(unreadable)?;
    // The error says where, never what: the file holds secrets.
    let json: Value = serde_json::from_str(&text)
        .map_err(|e| Problem::Failed(format!("{} isn't valid JSON (line {})", path.display(), e.line())))?;
    let oauth = json.get("claudeAiOauth").ok_or(Problem::SignedOut)?;
    let access = oauth.get("accessToken").and_then(Value::as_str).filter(|token| !token.is_empty());
    let access = access.ok_or(Problem::SignedOut)?.to_owned();
    if let Some(expires_ms) = oauth.get("expiresAt").and_then(Value::as_i64)
        && expires_ms <= now.as_millisecond()
    {
        return Err(Problem::Expired);
    }
    let plan = oauth.get("subscriptionType").and_then(Value::as_str).and_then(plan_name);
    Ok(Token { access, plan })
}

/// `max` → `Max`.
fn plan_name(kind: &str) -> Option<String> {
    let mut chars = kind.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// Asks `url` for the limits.
pub fn fetch(url: &str, token: &Token) -> Result<Vec<UsageLimit>, Problem> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Authorization", &format!("Bearer {}", token.access))
        .header("anthropic-beta", BETA_HEADER)
        .call()
        .map_err(|e| Problem::Failed(e.to_string()))?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(Problem::Expired),
        429 => {
            let after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()?.trim().parse().ok())
                .map(Duration::from_secs);
            return Err(Problem::RateLimited(after));
        }
        status => return Err(Problem::Failed(format!("Anthropic answered {status}"))),
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_string()
        .map_err(|e| Problem::Failed(e.to_string()))?;
    let json: Value = serde_json::from_str(&body).map_err(|e| Problem::Failed(format!("unexpected answer: {e}")))?;
    Ok(limits_from_usage(&json))
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

    fn write(dir: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nova-claude-api-{dir}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".credentials.json");
        std::fs::write(&path, text).unwrap();
        set_mode(&path, 0o600);
        path
    }

    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_file_others_can_read_is_not_used() {
        let path = write("open", r#"{"claudeAiOauth":{"accessToken":"t"}}"#);
        set_mode(&path, 0o644);
        assert_eq!(read_token(&path, now()).err(), Some(Problem::Unsafe(path.clone())));
        assert!(Problem::Unsafe(path).hint().contains("chmod 600"));
    }

    #[test]
    fn the_checks_look_at_the_file_that_is_read() {
        let path = write("swapped", r#"{"claudeAiOauth":{"accessToken":"open"}}"#);
        set_mode(&path, 0o644);
        let opened = File::open(&path).unwrap();
        // A private file takes its place between the open and the checks.
        let private = path.with_extension("new");
        std::fs::write(&private, r#"{"claudeAiOauth":{"accessToken":"private"}}"#).unwrap();
        set_mode(&private, 0o600);
        std::fs::rename(&private, &path).unwrap();
        assert_eq!(token_from(opened, &path, now()).err(), Some(Problem::Unsafe(path.clone())));
    }

    #[test]
    fn reads_the_token_and_plan() {
        let path =
            write("ok", r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1792800000000,"subscriptionType":"max"}}"#);
        let token = read_token(&path, now()).unwrap();
        assert_eq!((token.access.as_str(), token.plan.as_deref()), ("t", Some("Max")));
    }

    #[test]
    fn an_expired_token_is_not_used() {
        let path = write("expired", r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1792700000000}}"#);
        assert_eq!(read_token(&path, now()).err(), Some(Problem::Expired));
    }

    #[test]
    fn no_file_or_no_claude_sign_in_means_signed_out() {
        assert_eq!(read_token(Path::new("/nonexistent/.credentials.json"), now()).err(), Some(Problem::SignedOut));
        let path = write("api-key", r#"{"other":{}}"#);
        assert_eq!(read_token(&path, now()).err(), Some(Problem::SignedOut));
    }

    #[test]
    fn a_broken_file_never_shows_its_contents() {
        let path = write("broken", r#"{"claudeAiOauth":{"accessToken":"secret""#);
        let Err(Problem::Failed(e)) = read_token(&path, now()) else { panic!() };
        assert!(!e.contains("secret"), "{e}");
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
