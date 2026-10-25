//! Starting an app from its desktop entry. Apps must outlive novad, and
//! some ask the kernel to kill them when their parent goes (bubblewrap's
//! `--die-with-parent`, which Claude Desktop's wrapper uses): an app is
//! never novad's child, nor a child of the thread that started it.
//!
//! With a systemd user manager each app runs as a service of its own
//! (`app-nova-<id>-<n>.service`, the freedesktop.org naming) that the
//! manager starts: it's not part of `nova.service`, and `systemctl --user`
//! shows it by name. Without one it's forked twice, so it ends up a child of
//! init, in a session of its own. Either way nothing it prints reaches novad.

use super::desktop_entry::{Entry, find_program};
use crate::logger::Logger;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const LOG_SOURCE: &str = "launcher";
/// How long a launch waits to hear the app started, and how often it looks.
const START_TIMEOUT: Duration = Duration::from_secs(2);
const START_POLL: Duration = Duration::from_millis(10);
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
    /// As services of their own, with this `systemd-run`.
    Service(PathBuf),
    /// Forked twice, in a session of their own.
    Direct,
}

impl Runner {
    /// A service when the user's systemd manager runs and `systemd-run` is on
    /// `path_var`.
    pub fn detect(path_var: &str) -> Self {
        let manager = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|runtime| Path::new(&runtime).join("systemd/private"))
            .is_some_and(|socket| socket.exists());
        match find_program("systemd-run", path_var) {
            Some(systemd_run) if manager => Runner::Service(systemd_run),
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
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // A folder that isn't there would fail the start.
    let dir = entry.path.clone().into_iter().chain(home).find(|dir| dir.is_dir()).unwrap_or_else(|| "/".into());
    let mut command = match runner {
        Runner::Service(systemd_run) => {
            let mut command = Command::new(systemd_run);
            command
                .args(["--user", "--quiet", "--collect", "--service-type=exec"])
                // What the app leaves running when its first process ends
                // (a launcher script's app) keeps running.
                .args([
                    "--property=KillMode=process",
                    "--property=StandardOutput=null",
                    "--property=StandardError=null",
                ])
                .arg(format!("--unit={}", unit_name(&entry.id)))
                .arg(format!("--working-directory={}", dir.display()));
            // novad's environment (Wayland's display, the session bus), by
            // name: systemd-run reads the values from its own. A name
            // systemd might refuse (a shell's `BASH_FUNC_x%%`) would fail
            // the start, so it's left out.
            for (name, _) in std::env::vars_os() {
                if let Some(name) =
                    name.to_str().filter(|name| plain_name(name) && !SERVICE_VARS.contains(name) && *name != "PATH")
                {
                    command.arg(format!("--setenv={name}"));
                }
            }
            // The session's PATH, below.
            // systemd expands `$NAME` in the command line again; doubled,
            // each `$` reaches the app as it was written.
            command
                .arg("--setenv=PATH")
                .arg("--")
                .arg(&program)
                .args(argv[1..].iter().map(|arg| arg.replace('$', "$$")));
            command
        }
        Runner::Direct => {
            let mut command = Command::new(&program);
            command.args(&argv[1..]);
            // Its own session, without novad's terminal: Ctrl+C on `nova
            // daemon` in a terminal doesn't reach it, and an app that
            // touches the terminal (Electron does) isn't stopped as a
            // background job of it. The fork's first half exits at once, and
            // the app waits for that before it execs: an app that asks to die
            // with its parent (bubblewrap's --die-with-parent) would die with
            // the first half if it asked first.
            // SAFETY: setsid, getpid, fork, getppid, sched_yield and _exit
            // are async-signal-safe and touch no memory.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let first_half = libc::getpid();
                    match libc::fork() {
                        -1 => Err(std::io::Error::last_os_error()),
                        0 => {
                            while libc::getppid() == first_half {
                                libc::sched_yield();
                            }
                            Ok(())
                        }
                        _ => libc::_exit(0),
                    }
                });
            }
            command
        }
    };
    for name in SERVICE_VARS {
        command.env_remove(name);
    }
    command.env("PATH", path_var).current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // What novad waits on: systemd-run, which returns once the app has
    // started or failed to, or the fork's first half, which returns at once.
    let mut child = command.spawn().map_err(|e| format!("can't start {}: {e}", program.display()))?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => return Err(format!("{} didn't start ({status})", program.display())),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(START_POLL),
            // Taking long: its answer is only logged.
            Ok(None) => {
                let logger = Arc::clone(logger);
                let id = entry.id.clone();
                std::thread::spawn(move || match child.wait() {
                    Ok(status) if !status.success() => logger.warn(LOG_SOURCE, &format!("{id} didn't start: {status}")),
                    Ok(_) => {}
                    Err(e) => logger.warn(LOG_SOURCE, &format!("lost track of {id}: {e}")),
                });
                break;
            }
            Err(e) => return Err(format!("lost track of {}: {e}", program.display())),
        }
    }
    logger.info(LOG_SOURCE, &format!("launched {}", entry.id));
    Ok(())
}

/// A name of letters, digits and `_`, not starting with a digit.
fn plain_name(name: &str) -> bool {
    !name.starts_with(|c: char| c.is_ascii_digit())
        && !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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

/// `app-nova-<id>-<n>.service`, the id without `.desktop` and escaped the way
/// systemd escapes unit names (`-` separates the parts, so it's `\x2d`
/// inside one), `n` different for every launch.
fn unit_name(id: &str) -> String {
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
    format!("app-nova-{escaped}-{}{n}.service", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_that_fails_to_start_says_so() {
        let dir = std::env::temp_dir().join(format!("novad-launch-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logger = Arc::new(Logger::open(&dir, nova_protocol::LogLevel::Info, false, Arc::default()).unwrap());
        let entry = Entry {
            id: "true.desktop".into(),
            name: "True".into(),
            generic_name: None,
            comment: None,
            icon: None,
            keywords: vec![],
            exec: vec!["true".into()],
            terminal: false,
            path: None,
            opens_types: false,
            window_class: None,
        };
        let path_var = std::env::var("PATH").unwrap_or_default();
        // A systemd-run that fails, as one does when the app can't start.
        let failing = find_program("false", &path_var).unwrap();
        let error = launch(&entry, &Runner::Service(failing), &path_var, &logger).unwrap_err();
        assert!(error.contains("didn't start"), "{error}");
        let started = find_program("true", &path_var).unwrap();
        assert_eq!(launch(&entry, &Runner::Service(started), &path_var, &logger), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_plain_variable_names_pass_to_the_service() {
        assert!(plain_name("WAYLAND_DISPLAY") && plain_name("_x1"));
        assert!(!plain_name("BASH_FUNC_x%%") && !plain_name("1A") && !plain_name(""));
    }

    #[test]
    fn unit_names_escape_the_id_and_never_repeat() {
        let first = unit_name("org.gnome.Nautilus.desktop");
        assert!(first.starts_with("app-nova-org.gnome.Nautilus-"), "{first}");
        assert!(first.ends_with(".service"));
        assert_ne!(unit_name("org.gnome.Nautilus.desktop"), first);
        assert!(unit_name("kde-dolphin.desktop").starts_with(r"app-nova-kde\x2ddolphin-"));
        assert!(unit_name("we ird/é.desktop").starts_with(r"app-nova-we\x20ird\x2f\xc3\xa9-"));
    }
}
