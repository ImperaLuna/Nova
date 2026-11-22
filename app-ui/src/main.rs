//! `nova-ui`: the program that draws Nova's UI with GPUI. novad runs it in
//! place of Quickshell when `features.toml` says `ui = "gpui"`, hands it
//! the socket (`NOVA_SOCKET`) and restarts it when it exits
//! (`app/src/supervisor.rs`).
//!
//! It starts GPUI with the look kit's icons as its assets, connects to novad
//! and draws nothing yet. The stage and each feature's ui crate plug in here
//! as the GPUI ports land (IMP-368). Until a feature shows novad's state,
//! `--probe` opens a window that does ([`probe`]), and `--replay` runs
//! against recordings where the real novad can't ([`replay`]).

mod args;
mod probe;
mod replay;

use args::Args;
use gpui::App;
use gpui_platform::application;
use nova_daemon_info::SOCKET_VAR;
use nova_look::IconAssets;
use nova_ui_client::Daemon;
use nova_ui_client::fake::FakeDaemon;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!("nova-ui: {problem}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };
    // novad logs each line the UI prints, and reads the level from a line
    // that opens with one (`nova_log`'s `ui_line`).
    eprintln!("INFO nova-ui {} started", env!("CARGO_PKG_VERSION"));
    let (socket, _replayed) = match socket(&args) {
        Ok(found) => found,
        Err(problem) => {
            eprintln!("ERROR nova-ui: {problem}");
            return ExitCode::FAILURE;
        }
    };
    application().with_assets(IconAssets).run(move |cx: &mut App| {
        Daemon::connect(socket, cx);
        if args.probe {
            probe::open(cx);
        }
    });
    ExitCode::SUCCESS
}

/// Where novad listens: the fake one's socket with `--replay` (it runs
/// until the value beside the path is dropped), else the one novad handed
/// over.
fn socket(args: &Args) -> Result<(PathBuf, Option<FakeDaemon>), String> {
    if let Some((socket, novad)) = replay::start(&args.replay)? {
        return Ok((socket, Some(novad)));
    }
    match std::env::var_os(SOCKET_VAR) {
        Some(socket) => Ok((socket.into(), None)),
        None => Err(format!(
            "{SOCKET_VAR} isn't set. novad sets it when it starts nova-ui; by hand, set it to novad's socket \
             or pass --replay."
        )),
    }
}
