//! Starting, restarting and stopping Nova: through the systemd user service
//! when it's installed, otherwise by running novad directly.

use crate::client::Client;
use crate::{status, systemd};
use nova_protocol::Command;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// How long to wait for novad to come up or go away.
const WAIT_LIMIT: Duration = Duration::from_secs(10);
const WAIT_STEP: Duration = Duration::from_millis(100);
/// The hidden subcommand that runs the daemon.
const DAEMON_COMMAND: &str = "daemon";
/// What to run to restart Nova, for hints.
pub const RESTART_COMMAND: &str = "nova restart";

/// `nova`: starts Nova, or shows its status when it already runs.
pub fn start(socket: &Path) -> Result<(), String> {
    if Client::connect(socket).is_ok() {
        println!("Nova is already running. To restart it, run `{RESTART_COMMAND}`.\n");
        return status::show(socket, false);
    }
    if systemd::unit_installed() {
        systemd::start()?;
    } else {
        spawn_novad(socket)?;
    }
    wait_until("novad to start", || Client::connect(socket).is_ok())?;
    println!("Nova started.\n");
    status::show(socket, false)
}

pub fn restart(socket: &Path) -> Result<(), String> {
    if systemd::unit_active() {
        systemd::restart()?;
        wait_until("novad to start", || Client::connect(socket).is_ok())?;
        println!("Nova restarted.\n");
        return status::show(socket, false);
    }
    stop_direct(socket)?;
    start(socket)
}

pub fn quit(socket: &Path) -> Result<(), String> {
    if systemd::unit_active() {
        systemd::stop()?;
    } else if !stop_direct(socket)? {
        println!("Nova isn't running.");
        return Ok(());
    }
    println!("Nova stopped.");
    Ok(())
}

/// Asks a novad that systemd doesn't manage to quit. False if none runs.
fn stop_direct(socket: &Path) -> Result<bool, String> {
    let Ok(mut client) = Client::connect(socket) else { return Ok(false) };
    client.request(Command::Quit)?;
    wait_until("novad to stop", || Client::connect(socket).is_err())?;
    Ok(true)
}

/// Starts the daemon (`nova daemon`) on `socket`, detached from this
/// terminal. It writes its own log, so its output is dropped.
fn spawn_novad(socket: &Path) -> Result<(), String> {
    let nova = std::env::current_exe().map_err(|e| format!("can't find the nova binary: {e}"))?;
    std::process::Command::new(&nova)
        .arg0(novad::PROCESS_NAME)
        .arg(DAEMON_COMMAND)
        .arg("--socket")
        .arg(socket)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("can't start {} {DAEMON_COMMAND}: {e}", nova.display()))
}

/// Waits for `done`, checking every [`WAIT_STEP`]. The socket has no "ready"
/// event to wait on before novad listens, so this one-off wait polls.
fn wait_until(what: &str, done: impl Fn() -> bool) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < WAIT_LIMIT {
        if done() {
            return Ok(());
        }
        std::thread::sleep(WAIT_STEP);
    }
    Err(format!(
        "gave up waiting for {what} after {} s. Run `nova logs --no-follow`, or `nova daemon` in a terminal to see why.",
        WAIT_LIMIT.as_secs()
    ))
}
