mod config;
mod doctor;
mod host;
mod lifecycle;
mod login_apps;
mod setup;
mod statusline;
mod systemd;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use nova_cli::{ipc, logs, status, toggle, wallpaper};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "nova",
    version,
    about = "Nova, a desktop shell for Wayland",
    long_about = "Nova, a desktop shell for Wayland.\n\nRun `nova` with no command to start Nova, or to see its status when it's already running."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<NovaCommand>,
    #[arg(long, global = true, hide = true, default_value_os_t = nova_paths::socket())]
    socket: PathBuf,
}

#[derive(Subcommand)]
enum NovaCommand {
    Restart,
    Quit,
    Status {
        #[arg(long)]
        json: bool,
    },
    Logs(logs::LogsArgs),
    Version,
    ClaudeStatusline(statusline::StatuslineArgs),
    #[command(hide = true)]
    Daemon(Box<novad::Args>),
    Setup(setup::SetupArgs),
    Doctor,
    Config(config::ConfigArgs),
    Completions {
        shell: clap_complete::Shell,
    },
    Toggle(toggle::ToggleArgs),
    Lock,
    Volume(toggle::VolumeArgs),
    Screenshot(toggle::ScreenshotArgs),
    Record,
    Wallpaper(wallpaper::WallpaperArgs),
    Ipc(ipc::IpcArgs),
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
        Some(NovaCommand::Config(args)) => config::run(&args),
        Some(NovaCommand::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "nova", &mut std::io::stdout());
            Ok(())
        }
        Some(NovaCommand::Ipc(args)) => ipc::run(&args, &socket),
        Some(NovaCommand::Version) => {
            status::version(&socket);
            Ok(())
        }
        Some(NovaCommand::ClaudeStatusline(args)) => match statusline::run(&args, &socket) {
            Ok(code) => return code,
            Err(e) => Err(e),
        },
        Some(NovaCommand::Toggle(args)) => toggle::run(&args, &socket),
        Some(NovaCommand::Lock) => toggle::lock(&socket),
        Some(NovaCommand::Volume(args)) => toggle::volume(&args, &socket),
        Some(NovaCommand::Screenshot(args)) => toggle::screenshot(&args, &socket),
        Some(NovaCommand::Record) => toggle::record(&socket),
        Some(NovaCommand::Wallpaper(args)) => wallpaper::run(&args, &socket),
        Some(NovaCommand::Daemon(args)) => novad::run(*args, socket).map_err(|e| format!("daemon: {e}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("nova: {message}");
            ExitCode::FAILURE
        }
    }
}
