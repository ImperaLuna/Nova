//! `nova`: Nova's one binary. Its commands start, stop and inspect Nova by
//! talking to the daemon, which is this same binary run as `nova daemon`;
//! the daemon's logic lives in the novad crate.

mod client;
mod lifecycle;
mod logs;
mod status;
mod systemd;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "nova",
    version,
    about = "Nova, a desktop shell for Wayland",
    long_about = "Nova, a desktop shell for Wayland.\n\nRun `nova` with no command to start Nova, or to see its status when it's already running.",
    after_help = "Commands marked [planned] are listed so you know they're coming; they don't work yet."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<NovaCommand>,
    /// novad's socket, if it isn't the default.
    #[arg(long, global = true, hide = true, default_value_os_t = nova_paths::socket())]
    socket: PathBuf,
}

#[derive(Subcommand)]
enum NovaCommand {
    /// Restart novad and the UI.
    Restart,
    /// Stop the UI and novad.
    Quit,
    /// Show what's running, its memory use and each feature's state.
    Status {
        /// Print novad's answer as JSON, for scripts.
        #[arg(long)]
        json: bool,
    },
    /// Show novad's and the UI's logs, then follow new lines.
    Logs(logs::LogsArgs),
    /// Print the version of nova.
    Version,
    /// Run Nova's daemon, which runs the UI. systemd and `nova` start it.
    #[command(hide = true)]
    Daemon(novad::Args),
    /// [planned] Install packages for your distro and GPU, the default config and the novad service.
    Setup,
    /// [planned] Check the compositor, Quickshell, novad, packages and config, and report what's wrong.
    Doctor,
    /// [planned] Print the config folder, or open it in your editor.
    Config,
    /// [planned] Print shell completions for bash, zsh or fish.
    Completions,
    /// [planned] Open or close a panel, such as the launcher. Meant for keybinds.
    Toggle,
    /// [planned] Send a raw request to novad and print the reply, for scripts.
    Ipc,
}

fn main() -> ExitCode {
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let Cli { command, socket } = cli;
    let result = match command {
        None => lifecycle::start(&socket),
        Some(NovaCommand::Restart) => lifecycle::restart(&socket),
        Some(NovaCommand::Quit) => lifecycle::quit(&socket),
        Some(NovaCommand::Status { json }) => status::show(&socket, json),
        Some(NovaCommand::Logs(args)) => logs::run(&args, &socket),
        Some(NovaCommand::Version) => {
            status::version(&socket);
            Ok(())
        }
        Some(NovaCommand::Daemon(args)) => novad::run(args, socket).map_err(|e| format!("daemon: {e}")),
        Some(
            NovaCommand::Setup
            | NovaCommand::Doctor
            | NovaCommand::Config
            | NovaCommand::Completions
            | NovaCommand::Toggle
            | NovaCommand::Ipc,
        ) => Err(format!(
            "`nova {}` isn't built yet; it's planned for a later version.",
            matches.subcommand_name().unwrap_or_default()
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("nova: {message}");
            ExitCode::FAILURE
        }
    }
}
