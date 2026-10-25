//! Starting an app from its desktop entry. With a systemd user manager each
//! app runs in a scope of its own (`app-nova-<id>-<n>.scope`, the
//! freedesktop.org naming), so it's not part of `nova.service`: restarting
//! or stopping Nova leaves it running, and `systemctl --user` shows it by
//! name. Without one it starts in a process group of its own. Either way
//! novad waits on it, so it never lingers as a zombie, and nothing it prints
//! reaches novad.

use super::desktop_entry::{Entry, find_program};
use crate::logger::Logger;
use nova_protocol::LogLevel;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const LOG_SOURCE: &str = "launcher";
/// Runs a command in a terminal, the way the freedesktop.org proposal says.
const TERMINAL_EXEC: &str = "xdg-terminal-exec";
/// The user's terminal, for systems without xdg-terminal-exec.
const TERMINAL_VAR: &str = "TERMINAL";
/// What systemd sets for novad itself, which an app mustn't take for its own.
const SERVICE_VARS: [&str; 7] =
    ["NOTIFY_SOCKET", "INVOCATION_ID", "JOURNAL_STREAM", "MANAGERPID", "SYSTEMD_EXEC_PID", "LISTEN_FDS", "LISTEN_PID"];

/// How apps are started.
#[derive(Clone, Debug)]
pub enum Runner {
    /// In a scope of their own, with this `systemd-run`.
    Scope(PathBuf),
    /// As novad's children, in a process group of their own.
    Direct,
}

impl Runner {
    /// A scope when the user's systemd manager runs and `systemd-run` is on
    /// `path_var`.
    pub fn detect(path_var: &str) -> Self {
        let manager = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|runtime| Path::new(&runtime).join("systemd/private"))
            .is_some_and(|socket| socket.exists());
        match find_program("systemd-run", path_var) {
            Some(systemd_run) if manager => Runner::Scope(systemd_run),
            _ => Runner::Direct,
        }
    }
}

/// Starts `entry`'s app.
pub fn launch(entry: &Entry, runner: &Runner, path_var: &str, logger: &Arc<Logger>) -> Result<(), String> {
    let mut argv = entry.exec.clone();
    if entry.terminal {
        argv.splice(0..0, terminal(path_var)?);
    }
    let program = find_program(&argv[0], path_var).ok_or_else(|| format!("{} isn't installed", argv[0]))?;
    let mut command = match runner {
        Runner::Scope(systemd_run) => {
            let mut command = Command::new(systemd_run);
            command
                .args(["--user", "--scope", "--quiet", "--collect", "--unit"])
                .arg(scope_name(&entry.id))
                .arg("--")
                .arg(&program)
                .args(&argv[1..]);
            command
        }
        Runner::Direct => {
            let mut command = Command::new(&program);
            command.args(&argv[1..]);
            command
        }
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // A folder that isn't there would fail the start.
    let dir = entry.path.clone().into_iter().chain(home).find(|dir| dir.is_dir()).unwrap_or_else(|| "/".into());
    for name in SERVICE_VARS {
        command.env_remove(name);
    }
    let mut child = command
        .env("PATH", path_var)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own group: a signal to novad's group (Ctrl+C on `nova daemon`
        // in a terminal) doesn't reach it.
        .process_group(0)
        .spawn()
        .map_err(|e| format!("can't start {}: {e}", program.display()))?;
    logger.info(LOG_SOURCE, &format!("launched {} (pid {})", entry.id, child.id()));
    let logger = Arc::clone(logger);
    let id = entry.id.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) if !status.success() => logger.log(LogLevel::Debug, LOG_SOURCE, &format!("{id} exited: {status}")),
        Ok(_) => {}
        Err(e) => logger.warn(LOG_SOURCE, &format!("lost track of {id}: {e}")),
    });
    Ok(())
}

/// What runs a terminal app: xdg-terminal-exec, else `$TERMINAL -e`.
fn terminal(path_var: &str) -> Result<Vec<String>, String> {
    if find_program(TERMINAL_EXEC, path_var).is_some() {
        return Ok(vec![TERMINAL_EXEC.into()]);
    }
    match std::env::var(TERMINAL_VAR) {
        Ok(terminal) if !terminal.trim().is_empty() => Ok(vec![terminal.trim().into(), "-e".into()]),
        _ => Err(format!("it runs in a terminal: install {TERMINAL_EXEC} or set {TERMINAL_VAR}")),
    }
}

/// `app-nova-<id>-<n>.scope`, the id without `.desktop` and escaped the way
/// systemd escapes unit names (`-` separates the parts, so it's `\x2d`
/// inside one), `n` different for every launch.
fn scope_name(id: &str) -> String {
    static LAUNCHES: AtomicU64 = AtomicU64::new(0);
    let app = id.strip_suffix(".desktop").unwrap_or(id);
    let mut escaped = String::with_capacity(app.len());
    for byte in app.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b':' | b'_' | b'.' => escaped.push(char::from(byte)),
            _ => escaped.push_str(&format!("\\x{byte:02x}")),
        }
    }
    let n = LAUNCHES.fetch_add(1, Ordering::Relaxed);
    format!("app-nova-{escaped}-{}{n}.scope", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_names_escape_the_id_and_never_repeat() {
        let first = scope_name("org.gnome.Nautilus.desktop");
        assert!(first.starts_with("app-nova-org.gnome.Nautilus-"), "{first}");
        assert!(first.ends_with(".scope"));
        assert_ne!(scope_name("org.gnome.Nautilus.desktop"), first);
        assert!(scope_name("kde-dolphin.desktop").starts_with(r"app-nova-kde\x2ddolphin-"));
        assert!(scope_name("we ird/é.desktop").starts_with(r"app-nova-we\x20ird\x2f\xc3\xa9-"));
    }
}
