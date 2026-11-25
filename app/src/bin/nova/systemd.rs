use std::process::{Command, Output};
use std::time::Duration;

pub const UNIT: &str = "nova.service";
// A little over systemd's 90 s default start and stop timeouts:
// https://www.freedesktop.org/software/systemd/man/latest/systemd-system.conf.html#DefaultTimeoutStartSec=
const JOB_TIMEOUT: Duration = Duration::from_secs(100);
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

const SESSION_VARS: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "NIRI_SOCKET",
    "HYPRLAND_INSTANCE_SIGNATURE",
];

pub fn unit_installed() -> bool {
    systemctl(&["show", "--property=LoadState", "--value", UNIT], QUERY_TIMEOUT)
        .is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "loaded")
}

pub fn unit_active() -> bool {
    systemctl(&["is-active", "--quiet", UNIT], QUERY_TIMEOUT).is_ok_and(|output| output.status.success())
}

pub fn start() -> Result<(), String> {
    import_session_env()?;
    run(&["start", UNIT], JOB_TIMEOUT)
}

pub fn restart() -> Result<(), String> {
    import_session_env()?;
    run(&["restart", UNIT], JOB_TIMEOUT)
}

pub fn stop() -> Result<(), String> {
    run(&["stop", UNIT], JOB_TIMEOUT)
}

fn import_session_env() -> Result<(), String> {
    let present: Vec<&str> = SESSION_VARS.iter().copied().filter(|var| std::env::var_os(var).is_some()).collect();
    if present.is_empty() {
        return Ok(());
    }
    run(&[&["import-environment"], present.as_slice()].concat(), QUERY_TIMEOUT)
}

fn run(args: &[&str], timeout: Duration) -> Result<(), String> {
    let output = systemctl(args, timeout)?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!("systemctl --user {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()))
}

fn systemctl(args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut command = Command::new("systemctl");
    command.arg("--user").args(args);
    novad::checks::output_within(command, timeout).map_err(|e| format!("can't run systemctl: {e}"))
}
