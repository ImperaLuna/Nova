//! `nova`: Nova's one binary. Its commands start, stop and inspect Nova by
//! talking to the daemon, which is this same binary run as `nova daemon`;
//! the daemon's logic lives in the novad crate.

/// Nova's GitHub repository (`owner/name`), as a literal so messages can
/// `concat!` it.
macro_rules! github_repo {
    () => {
        "ImperaLuna/Nova"
    };
}

mod client;
mod config;
mod doctor;
mod host;
mod ipc;
mod lifecycle;
mod login_apps;
mod logs;
mod setup;
mod status;
mod statusline;
mod systemd;
mod toggle;
mod wallpaper;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
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
    /// Print the config folder, or open config.toml in your editor.
    Config(config::ConfigArgs),
    /// Print shell completions, to load from your shell's startup file.
    ///
    /// bash: `source <(nova completions bash)`; zsh: `source <(nova completions zsh)`;
    /// fish: `nova completions fish | source`.
    Completions { shell: clap_complete::Shell },
    /// Open or close a panel on the focused output. Meant for keybinds.
    Toggle(toggle::ToggleArgs),
    /// Lock the screen; your password unlocks it. Meant for keybinds and idle daemons.
    Lock,
    /// Change the volume and show Nova's volume capsule. Meant for the volume keys.
    ///
    /// Bind your compositor's volume keys to `nova volume up`, `nova volume down`
    /// and `nova volume mute` (instead of `wpctl`), so the capsule also shows at
    /// 0%, at 100% and when there's no output.
    Volume(toggle::VolumeArgs),
    /// Take a screenshot: saved to the screenshot folder and copied.
    ///
    /// Bind `nova screenshot region` to Super+Shift+S and `nova screenshot screen`
    /// to Print. The folder is `[screenshot] folder` in config.toml.
    Screenshot(toggle::ScreenshotArgs),
    /// Record the screen, or stop recording.
    ///
    /// Opens the picker to choose a region or a screen and the sound; pressing
    /// it again stops and saves the recording. Bind it to Super+Shift+R. The
    /// folder is `[recording] folder` in config.toml.
    Record,
    /// Set the wallpaper on every screen, or one; without a file, show what's set.
    ///
    /// An image fades in. A video, a Wallpaper Engine project folder or a
    /// Steam Workshop ID moves, with the `wallpaper-engine` feature turned
    /// on. It's remembered until you change `[wallpaper]` in config.toml,
    /// which sets the wallpapers Nova starts with.
    Wallpaper(wallpaper::WallpaperArgs),
    /// Send a raw request to novad and print the reply as JSON, for scripts.
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
