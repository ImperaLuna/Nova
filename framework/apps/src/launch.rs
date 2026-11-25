use crate::desktop_entry::Entry;
use nova_log::Logger;
use nova_paths::find_program;
use nova_process::child;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const LOG_SOURCE: &str = "launcher";
const START_TIMEOUT: Duration = Duration::from_secs(2);
const TERMINAL_EXEC: &str = "xdg-terminal-exec";
const TERMINAL_VAR: &str = "TERMINAL";
const SERVICE_VARS: [&str; 7] = [
    nova_process::notify::SOCKET_VAR,
    "INVOCATION_ID",
    "JOURNAL_STREAM",
    "MANAGERPID",
    "SYSTEMD_EXEC_PID",
    "LISTEN_FDS",
    "LISTEN_PID",
];

#[derive(Clone, Debug)]
pub enum Runner {
    Service(PathBuf),
    Direct,
}

impl Runner {
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

pub fn launch(entry: &Entry, runner: &Runner, path_var: &str, logger: &Arc<Logger>) -> Result<(), String> {
    start(entry, entry.exec.iter().map(OsString::from).collect(), runner, path_var, logger)
}

fn start(
    entry: &Entry,
    mut argv: Vec<OsString>,
    runner: &Runner,
    path_var: &str,
    logger: &Arc<Logger>,
) -> Result<(), String> {
    if entry.terminal {
        argv.splice(0..0, terminal(path_var)?.into_iter().map(OsString::from));
    }
    let name = argv[0].to_string_lossy().into_owned();
    let program = find_program(&name, path_var).ok_or_else(|| format!("{name} isn't installed"))?;
    let home = nova_paths::home();
    let dir = entry.path.clone().into_iter().chain(home).find(|dir| dir.is_dir()).unwrap_or_else(|| "/".into());
    let mut command = match runner {
        Runner::Service(systemd_run) => {
            let mut command = Command::new(systemd_run);
            command
                .args(["--user", "--quiet", "--collect", "--service-type=exec"])
                .args([
                    "--property=KillMode=process",
                    "--property=StandardOutput=null",
                    "--property=StandardError=null",
                ])
                .arg(format!("--unit={}", unit_name(&entry.id)))
                .arg(format!("--working-directory={}", dir.display()));
            // systemd refuses an environment name that isn't a plain variable name (a shell's `BASH_FUNC_x%%`) and the start fails:
            // https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html#Environment=
            for (name, _) in std::env::vars_os() {
                if let Some(name) =
                    name.to_str().filter(|name| plain_name(name) && !SERVICE_VARS.contains(name) && *name != "PATH")
                {
                    command.arg(format!("--setenv={name}"));
                }
            }
            // systemd expands `$NAME` in a command line again, so each `$` is doubled:
            // https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Command%20lines
            command.arg("--setenv=PATH").arg("--").arg(&program).args(argv[1..].iter().map(|arg| doubled_dollars(arg)));
            command
        }
        Runner::Direct => {
            let mut command = Command::new(&program);
            command.args(&argv[1..]);
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
                            // An app's own PR_SET_PDEATHSIG (bubblewrap's --die-with-parent) set before this would fire as the first half exits:
                            // https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html
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
    let mut child = command.spawn().map_err(|e| format!("can't start {}: {e}", program.display()))?;
    match child::wait_within(&mut child, START_TIMEOUT) {
        Ok(Some(status)) if status.success() => {}
        Ok(Some(status)) => return Err(format!("{} didn't start ({status})", program.display())),
        Ok(None) => {
            let logger = Arc::clone(logger);
            let id = entry.id.clone();
            std::thread::spawn(move || match child.wait() {
                Ok(status) if !status.success() => logger.warn(LOG_SOURCE, &format!("{id} didn't start: {status}")),
                Ok(_) => {}
                Err(e) => logger.warn(LOG_SOURCE, &format!("lost track of {id}: {e}")),
            });
        }
        Err(e) => return Err(format!("lost track of {}: {e}", program.display())),
    }
    logger.info(LOG_SOURCE, &format!("launched {}", entry.id));
    Ok(())
}

const OPENER: &str = "xdg-open";

pub fn open(target: &Path, path_var: &str, logger: &Arc<Logger>) -> Result<(), String> {
    let opener = Entry {
        id: format!("{OPENER}.desktop"),
        name: OPENER.into(),
        generic_name: None,
        comment: None,
        icon: None,
        keywords: Vec::new(),
        exec: vec![OPENER.into(), target.display().to_string()],
        terminal: false,
        path: None,
        mime_types: vec![],
        listed: true,
        window_class: None,
        categories: vec![],
    };
    launch(&opener, &Runner::detect(path_var), path_var, logger)
}

fn doubled_dollars(arg: &OsStr) -> OsString {
    let mut doubled = Vec::with_capacity(arg.len());
    for &byte in arg.as_bytes() {
        doubled.push(byte);
        if byte == b'$' {
            doubled.push(b'$');
        }
    }
    OsString::from_vec(doubled)
}

fn plain_name(name: &str) -> bool {
    !name.starts_with(|c: char| c.is_ascii_digit())
        && !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn terminal(path_var: &str) -> Result<Vec<String>, String> {
    if find_program(TERMINAL_EXEC, path_var).is_some() {
        return Ok(vec![TERMINAL_EXEC.into()]);
    }
    match std::env::var(TERMINAL_VAR) {
        Ok(terminal) if !terminal.trim().is_empty() => Ok(vec![terminal.trim().into(), "-e".into()]),
        _ => Err(format!("it runs in a terminal: install {TERMINAL_EXEC} or set {TERMINAL_VAR}")),
    }
}

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
        let dir = nova_test_support::TempDir::new("launch-fail");
        let logger = Arc::new(Logger::open(&dir, nova_carrier::log::LogLevel::Info, false, Arc::default()).unwrap());
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
            mime_types: vec![],
            listed: true,
            window_class: None,
            categories: vec![],
        };
        let path_var = std::env::var("PATH").unwrap_or_default();
        let failing = find_program("false", &path_var).unwrap();
        let error = launch(&entry, &Runner::Service(failing), &path_var, &logger).unwrap_err();
        assert!(error.contains("didn't start"), "{error}");
        let started = find_program("true", &path_var).unwrap();
        assert_eq!(launch(&entry, &Runner::Service(started), &path_var, &logger), Ok(()));
    }

    #[test]
    fn only_plain_variable_names_pass_to_the_service() {
        assert!(plain_name("WAYLAND_DISPLAY") && plain_name("_x1"));
        assert!(!plain_name("BASH_FUNC_x%%") && !plain_name("1A") && !plain_name(""));
    }

    #[test]
    fn dollars_are_doubled_byte_for_byte() {
        let arg = OsStr::from_bytes(b"/tmp/caf\xe9 $HOME.png");
        assert_eq!(doubled_dollars(arg).as_bytes(), b"/tmp/caf\xe9 $$HOME.png");
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
