//! `nova`: Nova's one binary. Its commands start, stop and inspect Nova by
//! talking to the daemon, which is this same binary run as `nova daemon`;
//! the daemon's logic lives in the novad crate.

mod client;
mod doctor;
mod host;
mod lifecycle;
mod logs;
mod setup;
mod status;
mod statusline;
mod systemd;
mod toggle;

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
    /// Claude Code's status line command: passes Claude's plan limits to the
    /// AI usage widget. Set it in Claude Code's settings as
    /// `nova claude-statusline`, or `nova claude-statusline -- <your command>`
    /// to keep a status line you already have.
    ClaudeStatusline(statusline::StatuslineArgs),
    /// Run Nova's daemon, which runs the UI. systemd and `nova` start it.
    #[command(hide = true)]
    Daemon(Box<novad::Args>),
    /// Install packages for your distro and GPU, the default config and the novad service.
    ///
    /// Shows its plan and asks before changing anything. Run it again after
    /// turning a feature on. On NixOS, use the flake instead.
    Setup(setup::SetupArgs),
    /// Check the compositor, Quickshell, novad, packages and config, and report what's wrong.
    ///
    /// Exits non-zero when a check fails; warnings alone don't.
    Doctor,
    /// [planned] Print the config folder, or open it in your editor.
    Config,
    /// [planned] Print shell completions for bash, zsh or fish.
    Completions,
    /// Open or close a panel on the focused output. Meant for keybinds.
    Toggle(toggle::ToggleArgs),
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
        Some(NovaCommand::Doctor) => doctor::run(&socket),
        Some(NovaCommand::Setup(args)) => setup::run(&args),
        Some(NovaCommand::Version) => {
            status::version(&socket);
            Ok(())
        }
        Some(NovaCommand::ClaudeStatusline(args)) => match statusline::run(&args, &socket) {
            Ok(code) => return code,
            Err(e) => Err(e),
        },
        Some(NovaCommand::Toggle(args)) => toggle::run(&args, &socket),
        Some(NovaCommand::Daemon(args)) => novad::run(*args, socket).map_err(|e| format!("daemon: {e}")),
        Some(NovaCommand::Config | NovaCommand::Completions | NovaCommand::Ipc) => Err(format!(
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
