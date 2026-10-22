//! The novad systemd user service, through `systemctl --user`.

use std::process::{Command, Output};

pub const UNIT: &str = "novad.service";

/// Session variables novad and the UI need. A compositor started without
/// systemd (Hyprland without uwsm) never hands them to systemd itself.
const SESSION_VARS: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "NIRI_SOCKET",
    "HYPRLAND_INSTANCE_SIGNATURE",
];

/// True when a systemd user manager runs and knows the novad unit.
pub fn unit_installed() -> bool {
    systemctl(&["show", "--property=LoadState", "--value", UNIT])
        .is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "loaded")
}

pub fn unit_active() -> bool {
    systemctl(&["is-active", "--quiet", UNIT]).is_ok_and(|output| output.status.success())
}

/// Copies this session's variables into the user manager, then starts novad.
pub fn start() -> Result<(), String> {
    import_session_env()?;
    run(&["start", UNIT])
}

pub fn restart() -> Result<(), String> {
    import_session_env()?;
    run(&["restart", UNIT])
}

pub fn stop() -> Result<(), String> {
    run(&["stop", UNIT])
}

fn import_session_env() -> Result<(), String> {
    let present: Vec<&str> = SESSION_VARS.iter().copied().filter(|var| std::env::var_os(var).is_some()).collect();
    if present.is_empty() {
        return Ok(());
    }
    run(&[&["import-environment"], present.as_slice()].concat())
}

fn run(args: &[&str]) -> Result<(), String> {
    let output = systemctl(args)?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!("systemctl --user {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()))
}

fn systemctl(args: &[&str]) -> Result<Output, String> {
    Command::new("systemctl").arg("--user").args(args).output().map_err(|e| format!("can't run systemctl: {e}"))
}
