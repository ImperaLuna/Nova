//! Where Nova keeps things. novad and the `nova` CLI both resolve paths here,
//! so the two always agree.
//!
//! `ui/daemon/Daemon.qml` derives the socket path on its own (QML can't call
//! Rust); keep it in step with [`socket`].

use std::path::{Path, PathBuf};

/// `$XDG_RUNTIME_DIR/nova/novad.sock`, or `/tmp/nova-<uid>/novad.sock` without it.
pub fn socket() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/nova-{}", current_uid())));
    base.join("nova").join("novad.sock")
}

/// `$XDG_CONFIG_HOME/nova`, else `~/.config/nova`.
pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("nova")
}

/// `$XDG_STATE_HOME/nova`, else `~/.local/state/nova`. Logs live here.
pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("nova")
}

/// `<prefix>/share/nova/<name>` next to the running binary (the Nix and
/// distro package layout) when it exists, else `./<name>` for a checkout.
pub fn installed_or_checkout(name: &str) -> PathBuf {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(|prefix| prefix.join("share/nova").join(name)));
    match installed {
        Some(dir) if dir.is_dir() => dir,
        _ => PathBuf::from(name),
    }
}

fn xdg_dir(var: &str, home_fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(home_fallback)))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn current_uid() -> String {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid().to_string()).unwrap_or_else(|_| "unknown".to_owned())
}

/// novad's current log inside a log dir (normally [`state_dir`]).
pub fn log_file(log_dir: &Path) -> PathBuf {
    log_dir.join(LOG_FILE_NAME)
}

/// The log of an earlier novad run: 1 is the run before the current one.
pub fn previous_log_file(log_dir: &Path, runs_ago: usize) -> PathBuf {
    log_dir.join(format!("{LOG_FILE_NAME}.{runs_ago}"))
}

/// Saved log tails from crashes of the UI or novad, one file each, named by
/// time so that sorting by name sorts by age.
pub fn crash_dir(log_dir: &Path) -> PathBuf {
    log_dir.join("crashes")
}

const LOG_FILE_NAME: &str = "nova.log";
