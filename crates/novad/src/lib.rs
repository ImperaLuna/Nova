//! novad: Nova's background daemon, run as `nova daemon`.
//!
//! It owns Nova's state, serves the socket that the UI and the `nova` CLI
//! talk to, runs the Quickshell UI as its child (restarting it when it
//! crashes) and keeps the log. Compositor adapters and services get added on
//! top of this.

mod ai_usage;
mod compositor;
mod config;
mod features;
mod hub;
mod inotify;
mod logger;
mod procfs;
mod server;
mod settings;
mod supervisor;
mod theme;

use hub::Hub;
use logger::Logger;
use nova_protocol::{LogLevel, Status, UiState, UiStatus};
use settings::{Settings, Sources};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use supervisor::{Supervisor, UiSettings};

/// The daemon's process name in ps and btop. It renames itself to this, and
/// whatever starts it passes it as `argv[0]` (`nova`, the systemd unit).
pub const PROCESS_NAME: &str = "Nova";
/// Sets the lowest level written to the log: debug, info (default), warn, error.
const LOG_LEVEL_VAR: &str = "NOVA_LOG";
const LOG_SOURCE: &str = "novad";

/// `nova daemon`'s options. The socket comes from `nova`'s own `--socket`.
#[derive(clap::Args)]
pub struct Args {
    /// Folder of feature manifests.
    #[arg(long, default_value_os_t = nova_features::default_dir())]
    features_dir: PathBuf,
    /// The user's feature choices. Reloaded when it changes.
    #[arg(long, default_value_os_t = nova_paths::config_dir().join("features.toml"))]
    selection: PathBuf,
    /// Folder of theme files (`<id>.toml`).
    #[arg(long, default_value_os_t = theme::default_dir())]
    themes_dir: PathBuf,
    /// The user's settings (see `config`). Reloaded when it changes.
    #[arg(long, default_value_os_t = nova_paths::config_dir().join("config.toml"))]
    config: PathBuf,
    /// The UI's QML folder.
    #[arg(long, default_value_os_t = nova_paths::installed_or_checkout("ui"))]
    ui: PathBuf,
    /// The Quickshell binary. By default the `nova-ui` launcher the package
    /// was built with or that's on PATH, else `quickshell`. Not its `qs` alias: on Nix, `qs` drops
    /// the `nova-ui` name novad gives the UI process.
    #[arg(long)]
    quickshell: Option<PathBuf>,
    /// Don't run the UI (tests, or running it by hand).
    #[arg(long)]
    no_ui: bool,
    /// Where nova.log and crash snapshots go.
    #[arg(long, default_value_os_t = nova_paths::state_dir())]
    log_dir: PathBuf,
    /// Where features keep what survives a restart.
    #[arg(long, default_value_os_t = nova_paths::state_dir())]
    state_dir: PathBuf,
    /// Codex's folder, read by the `ai-usage` feature. By default
    /// `$CODEX_HOME`, else `~/.codex`.
    #[arg(long)]
    codex_home: Option<PathBuf>,
    /// Claude Code's credentials file, read by the `ai-usage` feature. By
    /// default `$CLAUDE_CONFIG_DIR/.credentials.json`, else
    /// `~/.claude/.credentials.json`.
    #[arg(long)]
    claude_credentials: Option<PathBuf>,
}

/// Everything a connection needs to answer requests.
pub struct Daemon {
    started: Instant,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Option<Arc<Supervisor>>,
    socket: PathBuf,
    sources: Sources,
    /// Replaced on reload. Events about it are published under this lock,
    /// so a new subscriber's snapshot and later events arrive in order.
    settings: Mutex<Settings>,
    /// Following the compositor, when it's one novad knows.
    compositor: Option<Arc<compositor::Compositor>>,
}

impl Daemon {
    fn status(&self) -> Status {
        let disabled = UiStatus { state: UiState::Disabled, pid: None, pss_kib: None, restarts: 0, last_crash: None };
        Status {
            daemon: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            uptime_secs: self.started.elapsed().as_secs(),
            pss_kib: procfs::pss_kib(std::process::id()),
            quickshell: self.ui.as_ref().and_then(|ui| ui.quickshell_version()),
            ui: self.ui.as_ref().map_or(disabled, |ui| ui.status()),
            features: lock(&self.settings).features.clone(),
            log_dir: self.logger.dir().display().to_string(),
            desktop: self.compositor.as_ref().and_then(|compositor| compositor.desktop()),
        }
    }

    /// Reads the settings files again and applies what changed.
    fn reload(&self) {
        lock(&self.settings).reload(&self.sources, &self.hub, &self.logger);
    }

    /// Starts watching the settings files, reloading when they change.
    fn watch_settings(self: &Arc<Self>) {
        let daemon = Arc::clone(self);
        let files = vec![self.sources.config.clone(), self.sources.selection.clone()];
        settings::watch(files, Arc::clone(&self.logger), move || daemon.reload());
    }

    /// Stops the UI and exits novad. Exit code 0, so systemd leaves it off.
    fn quit(&self) -> ! {
        self.logger.info(LOG_SOURCE, "quit requested, stopping");
        if let Some(ui) = &self.ui {
            ui.stop();
        }
        let _ = std::fs::remove_file(&self.socket);
        std::process::exit(0)
    }
}

/// Runs novad until it quits.
pub fn run(args: Args, socket: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Before any thread starts, so they all inherit the name.
    set_process_name(PROCESS_NAME)?;
    let (listener, daemon) = start(args, socket)?;
    install_panic_hook(Arc::clone(&daemon.logger));
    server::serve(listener, daemon);
    Ok(())
}

/// Starts novad on background threads in this process and returns once its
/// socket accepts connections. For tests of novad's clients.
pub fn spawn(args: Args, socket: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let (listener, daemon) = start(args, socket)?;
    std::thread::spawn(move || server::serve(listener, daemon));
    Ok(())
}

fn start(
    args: Args,
    socket: PathBuf,
) -> Result<(std::os::unix::net::UnixListener, Arc<Daemon>), Box<dyn std::error::Error>> {
    // The socket first: a second daemon is refused before it can rotate the
    // running one's log.
    let listener = server::bind(&socket)?;
    let hub = Arc::new(Hub::default());
    let logger = Arc::new(Logger::open(&args.log_dir, log_level(), std::io::stderr().is_terminal(), Arc::clone(&hub))?);
    logger.info(LOG_SOURCE, &format!("novad {} starting (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));

    let path_var = session_path();
    let sources = Sources {
        config: args.config,
        selection: args.selection,
        themes_dir: args.themes_dir,
        manifests: nova_features::load_all(&args.features_dir)?,
        path_var: path_var.clone(),
        ai_usage: ai_usage::Paths::new(&args.state_dir, args.codex_home, args.claude_credentials),
    };
    let settings = Settings::load(&sources, &hub, &logger);

    logger.info(LOG_SOURCE, &format!("listening on {}", socket.display()));

    let ui = (!args.no_ui).then(|| {
        let wayland_socket = wayland_socket();
        if wayland_socket.is_none() {
            logger.warn(
                LOG_SOURCE,
                "WAYLAND_DISPLAY isn't set, so the UI can't open a window. If systemd started novad, \
                 run `nova` from your compositor's autostart so it can pass the session on.",
            );
        }
        let settings = UiSettings {
            quickshell: args.quickshell.clone().unwrap_or_else(|| supervisor::default_quickshell(&path_var)),
            ui_dir: args.ui.clone(),
            socket: socket.clone(),
            path_var,
            wayland_socket,
        };
        Supervisor::start(settings, Arc::clone(&logger))
    });

    let compositor = match compositor::detect(|name| std::env::var_os(name)) {
        Some(backend) => Some(compositor::Compositor::start(backend, Arc::clone(&hub), Arc::clone(&logger))),
        None => {
            logger.warn(
                LOG_SOURCE,
                "neither NIRI_SOCKET nor HYPRLAND_INSTANCE_SIGNATURE is set, so no workspaces will show",
            );
            None
        }
    };

    let daemon = Arc::new(Daemon {
        started: Instant::now(),
        hub,
        logger,
        ui,
        socket,
        sources,
        settings: Mutex::new(settings),
        compositor,
    });
    daemon.watch_settings();
    Ok((listener, daemon))
}

fn log_level() -> LogLevel {
    match std::env::var(LOG_LEVEL_VAR) {
        Ok(value) => value.parse().unwrap_or_else(|e| {
            eprintln!("novad: {LOG_LEVEL_VAR}: {e}; using info");
            LogLevel::Info
        }),
        Err(_) => LogLevel::Info,
    }
}

/// Logs a panic and saves a crash snapshot, then lets the default hook print
/// it to stderr (journald under systemd).
fn install_panic_hook(logger: Arc<Logger>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = logger.try_save_crash(&format!("novad panicked: {info}")) {
            eprintln!("novad: crash log saved to {}", path.display());
        }
        default_hook(info);
    }));
}

/// `PATH` plus `~/.local/bin`: systemd user services don't get the login
/// shell's PATH, which is where tools installed with pip or cargo live.
fn session_path() -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    let local_bin = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin"));
    match local_bin {
        Some(dir) if dir.is_dir() && !std::env::split_paths(&path).any(|entry| entry == dir) => {
            format!("{path}:{}", dir.display())
        }
        _ => path,
    }
}

/// Sets this thread's name, which is the process name ps and btop show for
/// the main thread (`/proc/<pid>/comm`).
fn set_process_name(name: &str) -> std::io::Result<()> {
    let name = std::ffi::CString::new(name).map_err(std::io::Error::other)?;
    // SAFETY: PR_SET_NAME reads a NUL-terminated string, which `name` is
    // for the whole call; the kernel copies at most 16 bytes of it.
    if unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr()) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The compositor's socket, from `WAYLAND_DISPLAY` (a name in
/// `XDG_RUNTIME_DIR`, or an absolute path).
fn wayland_socket() -> Option<PathBuf> {
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").filter(|value| !value.is_empty())?);
    if display.is_absolute() {
        return Some(display);
    }
    Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(display))
}

/// Locks `mutex` even if another thread panicked while holding it: novad's
/// state stays usable, and the panic is already logged.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Claude's limits from a Claude Code status line input, for
/// `nova claude-statusline`.
pub use ai_usage::limits_from_statusline as claude_limits_from_statusline;

#[cfg(test)]
mod tests;
