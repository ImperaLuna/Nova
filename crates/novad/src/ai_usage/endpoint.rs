//! What the usage endpoints (`claude_api`, `codex_api`) share: reading a
//! sign-in file only when it's private to the user, and one careful request.
//!
//! A sign-in's token goes nowhere but its request's header, sent to the
//! provider only (no redirects) under Nova's own user agent. It is never
//! refreshed: that would rotate the tool's sign-in under it. When it has
//! expired, the tool renews it the next time it runs.

use nova_protocol::AiProvider;
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
/// The answers are a few KiB at most.
const MAX_BODY_BYTES: u64 = 64 * 1024;
const USER_AGENT: &str = concat!("nova/", env!("CARGO_PKG_VERSION"));
/// Sign-in files are a few KiB; past this isn't read.
const MAX_SIGN_IN_BYTES: u64 = 64 * 1024;
/// Permission bits for anyone but the owner.
const GROUP_OTHER_BITS: u32 = 0o077;

#[derive(Debug, PartialEq)]
pub enum Problem {
    /// No sign-in file, or no subscription sign-in in it.
    SignedOut,
    /// The token has expired, or the provider refused it.
    Expired,
    /// The sign-in file isn't the user's, or others can read it.
    Unsafe(PathBuf),
    /// The provider asked to slow down, for this long if it said.
    RateLimited(Option<Duration>),
    Failed(String),
}

impl Problem {
    /// What the panel says about `provider` when there are no numbers.
    pub fn hint(&self, provider: AiProvider) -> String {
        let (tool, account, company, name) = match provider {
            AiProvider::Claude => ("Claude Code", "Claude", "Anthropic", "Claude"),
            AiProvider::Codex => ("Codex", "ChatGPT", "OpenAI", "Codex"),
        };
        match self {
            Problem::SignedOut => format!("Sign in to {tool} with your {account} account to see your limits."),
            Problem::Expired => format!("{tool}'s sign-in has expired. Open {tool} to renew it."),
            Problem::Unsafe(path) => format!(
                "Nova won't use {} because other users could read it or it isn't yours. To fix it, run\nchmod 600 {}",
                path.display(),
                path.display()
            ),
            Problem::RateLimited(_) => format!("{company} is busy; trying again in a while."),
            Problem::Failed(e) => format!("Can't get {name}'s limits: {e}"),
        }
    }
}

/// The JSON in the sign-in file at `path`, if it's there and private to the
/// user.
pub fn read_sign_in(path: &Path) -> Result<Value, Problem> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Problem::SignedOut),
        Err(e) => return Err(Problem::Failed(format!("can't read {}: {e}", path.display()))),
    };
    sign_in_from(file, path)
}

/// The JSON in `file`, opened from `path`. The checks look at the open file,
/// so a file swapped in at `path` after them is never read.
pub fn sign_in_from(file: File, path: &Path) -> Result<Value, Problem> {
    let unreadable = |e: std::io::Error| Problem::Failed(format!("can't read {}: {e}", path.display()));
    let metadata = file.metadata().map_err(unreadable)?;
    // SAFETY: geteuid has no preconditions and can't fail.
    let uid = unsafe { libc::geteuid() };
    if !metadata.is_file() {
        return Err(Problem::Failed(format!("{} isn't a file", path.display())));
    }
    if metadata.uid() != uid || metadata.mode() & GROUP_OTHER_BITS != 0 {
        return Err(Problem::Unsafe(path.to_owned()));
    }
    let mut text = String::new();
    file.take(MAX_SIGN_IN_BYTES).read_to_string(&mut text).map_err(unreadable)?;
    // The error says where, never what: the file holds secrets.
    serde_json::from_str(&text)
        .map_err(|e| Problem::Failed(format!("{} isn't valid JSON (line {})", path.display(), e.line())))
}

/// GETs `url` with `headers` and reads the JSON answer. `company` names the
/// provider in errors.
pub fn get_json(url: &str, headers: &[(&str, &str)], company: &str) -> Result<Value, Problem> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.call().map_err(|e| Problem::Failed(e.to_string()))?;
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
        status => return Err(Problem::Failed(format!("{company} answered {status}"))),
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_string()
        .map_err(|e| Problem::Failed(e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| Problem::Failed(format!("unexpected answer: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file written and the folder it's in, which goes when dropped.
    fn write(dir: &str, text: &str) -> (nova_test_support::TempDir, PathBuf) {
        let dir = nova_test_support::TempDir::new(&format!("endpoint-{dir}"));
        let path = dir.join("sign-in.json");
        std::fs::write(&path, text).unwrap();
        set_mode(&path, 0o600);
        (dir, path)
    }

    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_file_others_can_read_is_not_used() {
        let (_dir, path) = write("open", r#"{"token":"t"}"#);
        set_mode(&path, 0o644);
        assert_eq!(read_sign_in(&path).err(), Some(Problem::Unsafe(path.clone())));
        assert!(Problem::Unsafe(path).hint(AiProvider::Codex).contains("chmod 600"));
    }

    #[test]
    fn the_checks_look_at_the_file_that_is_read() {
        let (_dir, path) = write("swapped", r#"{"token":"open"}"#);
        set_mode(&path, 0o644);
        let opened = File::open(&path).unwrap();
        // A private file takes its place between the open and the checks.
        let private = path.with_extension("new");
        std::fs::write(&private, r#"{"token":"private"}"#).unwrap();
        set_mode(&private, 0o600);
        std::fs::rename(&private, &path).unwrap();
        assert_eq!(sign_in_from(opened, &path).err(), Some(Problem::Unsafe(path.clone())));
    }

    #[test]
    fn only_a_regular_file_is_read() {
        let dir = nova_test_support::TempDir::new("endpoint-dir");
        let path = dir.join("sign-in.json");
        std::fs::create_dir_all(&path).unwrap();
        set_mode(&path, 0o700);
        assert_eq!(read_sign_in(&path).err(), Some(Problem::Failed(format!("{} isn't a file", path.display()))));
    }

    #[test]
    fn no_file_means_signed_out() {
        assert_eq!(read_sign_in(Path::new("/nonexistent/sign-in.json")).err(), Some(Problem::SignedOut));
    }

    #[test]
    fn a_broken_file_never_shows_its_contents() {
        let (_dir, path) = write("broken", r#"{"token":"secret""#);
        let Err(Problem::Failed(e)) = read_sign_in(&path) else { panic!() };
        assert!(!e.contains("secret"), "{e}");
    }

    #[test]
    fn hints_name_the_provider() {
        assert_eq!(
            Problem::Expired.hint(AiProvider::Claude),
            "Claude Code's sign-in has expired. Open Claude Code to renew it."
        );
        assert_eq!(
            Problem::SignedOut.hint(AiProvider::Codex),
            "Sign in to Codex with your ChatGPT account to see your limits."
        );
    }
}
