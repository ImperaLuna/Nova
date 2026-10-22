//! `novad`: Nova's background daemon. `main.rs` only parses arguments.
//!
//! It owns Nova's state, serves the socket that the UI and the `nova` CLI
//! talk to, runs the Quickshell UI as its child (restarting it when it
//! crashes) and keeps the log. Compositor adapters and services get added on
//! top of this.

mod features;
mod hub;
mod logger;
mod procfs;
mod server;
mod supervisor;

use clap::Parser;
use hub::Hub;
use logger::Logger;
use nova_features::Selection;
use nova_protocol::{FeatureState, LogLevel, Status, UiState, UiStatus};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use supervisor::{Supervisor, UiSettings};

/// Sets the lowest level written to the log: debug, info (default), warn, error.
const LOG_LEVEL_VAR: &str = "NOVA_LOG";
const LOG_SOURCE: &str = "novad";

#[derive(Parser)]
#[command(name = "novad", version, about = "Nova's background daemon: runs the UI and serves the nova socket")]
pub struct Args {
    /// Socket to listen on.
    #[arg(long, default_value_os_t = nova_paths::socket())]
    socket: PathBuf,
    /// Folder of feature manifests.
    #[arg(long, default_value_os_t = nova_features::default_dir())]
    features_dir: PathBuf,
    /// The user's feature choices.
    #[arg(long, default_value_os_t = nova_paths::config_dir().join("features.toml"))]
    selection: PathBuf,
    /// The UI's QML folder.
    #[arg(long, default_value_os_t = nova_paths::installed_or_checkout("ui"))]
    ui: PathBuf,
    /// The Quickshell binary.
    #[arg(long, default_value = "qs")]
    quickshell: PathBuf,
    /// Don't run the UI (tests, or running it by hand).
    #[arg(long)]
    no_ui: bool,
    /// Where nova.log and crash snapshots go.
    #[arg(long, default_value_os_t = nova_paths::state_dir())]
    log_dir: PathBuf,
}

/// Everything a connection needs to answer requests.
pub struct Daemon {
    started: Instant,
    features: Vec<FeatureState>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Option<Arc<Supervisor>>,
    socket: PathBuf,
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
            features: self.features.clone(),
        }
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
pub fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let (listener, daemon) = start(args)?;
    install_panic_hook(Arc::clone(&daemon.logger));
    server::serve(listener, daemon);
    Ok(())
}

/// Starts novad on background threads in this process and returns once its
/// socket accepts connections. For tests of novad's clients.
pub fn spawn(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let (listener, daemon) = start(args)?;
    std::thread::spawn(move || server::serve(listener, daemon));
    Ok(())
}

fn start(args: Args) -> Result<(std::os::unix::net::UnixListener, Arc<Daemon>), Box<dyn std::error::Error>> {
    let hub = Arc::new(Hub::default());
    let logger = Arc::new(Logger::open(&args.log_dir, log_level(), std::io::stderr().is_terminal(), Arc::clone(&hub))?);
    logger.info(LOG_SOURCE, &format!("novad {} starting (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));

    let path_var = session_path();
    let selection = match std::fs::read_to_string(&args.selection) {
        Ok(text) => Selection::parse(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Selection::default(),
        Err(e) => return Err(format!("can't read {}: {e}", args.selection.display()).into()),
    };
    let features = features::states(&nova_features::load_all(&args.features_dir)?, &selection, &path_var);
    for feature in &features {
        logger.info("features", &format!("{} is {:?}", feature.id, feature.status));
    }

    let listener = server::bind(&args.socket)?;
    logger.info(LOG_SOURCE, &format!("listening on {}", args.socket.display()));

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
            quickshell: args.quickshell.clone(),
            ui_dir: args.ui.clone(),
            socket: args.socket.clone(),
            path_var,
            wayland_socket,
        };
        Supervisor::start(settings, Arc::clone(&logger))
    });

    let daemon = Arc::new(Daemon { started: Instant::now(), features, hub, logger, ui, socket: args.socket });
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

#[cfg(test)]
mod tests;
