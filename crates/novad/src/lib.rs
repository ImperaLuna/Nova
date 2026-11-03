//! novad: Nova's background daemon, run as `nova daemon`.
//!
//! It owns Nova's state, serves the socket that the UI and the `nova` CLI
//! talk to, runs the Quickshell UI as its child (restarting it when it
//! crashes) and keeps the log. Compositor adapters and services get added on
//! top of this.
//!
//! Adding a feature with its own daemon service and event topic touches, end
//! to end (`ai_usage` is the worked example):
//! - `features/<id>/feature.toml`: the feature, its dependencies, `daemon = "<id>"`.
//! - Its own module folder, with an `impl Module` (`modules`): how it
//!   starts, what its options are, what a new subscriber gets, how it stops.
//! - `modules/mod.rs`: one line in the `modules!` registry.
//! - `nova-protocol`: the `Topic`, the `Event` and any `Command`s, plus the
//!   wire format snapshot.
//! - `config.rs`: its `[<id>]` options in config.toml, if any.
//! - `server.rs`: its commands (through `Daemon::module`, which answers
//!   when the feature isn't ready) and its topic among the modules'.
//! - `just protocol-js` (regenerates the names in `ui/daemon/protocol.js`) and
//!   `Daemon.qml` (subscribe, keep the event's data).
//! - A QML service singleton beside the feature's components
//!   (`ui/ai_usage/service/AiUsageService.qml`, its own folder so QML tests
//!   can load the components without Quickshell) that reads that data and sends the
//!   commands, and the component that loads when the feature is ready.

mod ai_usage;
mod apps;
mod backoff;
mod bluetooth;
pub mod checks;
mod child;
mod compositor;
mod config;
mod egl;
mod features;
mod files;
mod host;
mod hub;
mod icons;
mod inotify;
mod launcher;
mod logger;
mod logind;
mod media;
mod mime;
mod modules;
mod network;
mod notifications;
mod notify;
mod peripherals;
mod portal_config;
mod power;
mod procfs;
mod recording;
mod render_loop;
mod reply_slot;
mod resources;
mod screenshot;
mod server;
mod session_bus;
mod session_lock;
mod settings;
mod state_file;
mod supervisor;
mod theme;
mod tray;
mod users;
mod wallpaper;
mod wallpaper_folder;
mod watch_points;
mod x11_tray;

use hub::Hub;
use logger::Logger;
use nova_protocol::{LogLevel, Status, UiState, UiStatus};
use settings::{Settings, Sources};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use supervisor::{Supervisor, UiSettings};

/// The daemon's process name in ps and btop. It renames itself to this, and
/// whatever starts it passes it as `argv[0]` (`nova`, the systemd unit).
pub const PROCESS_NAME: &str = "Nova";
/// Sets the lowest level written to the log: debug, info (default), warn, error.
const LOG_LEVEL_VAR: &str = "NOVA_LOG";
const LOG_SOURCE: &str = "novad";
/// Beside the socket, in `XDG_RUNTIME_DIR`: the login apps were started this
/// session (`--autostart`), named after the login session (`XDG_SESSION_ID`)
/// when there is one, since the folder outlives a graphical session while
/// another login of the user's (SSH) stays.
const AUTOSTART_MARKER: &str = "autostarted";

/// `nova daemon`'s options. The socket comes from `nova`'s own `--socket`.
#[derive(clap::Args)]
pub struct Args {
    /// Folder of feature manifests.
    #[arg(long, default_value_os_t = nova_features::default_dir())]
    features_dir: PathBuf,
    /// The user's feature choices. Reloaded when it changes. By default
    /// the user's features.toml, else the system's (`/etc/xdg/nova`).
    #[arg(long)]
    selection: Option<PathBuf>,
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
    /// The Claude desktop app's config, whose sign-in the `ai-usage`
    /// feature uses when Claude Code's file has none. By default
    /// `$XDG_CONFIG_HOME/Claude/config.json`, else `~/.config/Claude/config.json`.
    #[arg(long)]
    claude_desktop_config: Option<PathBuf>,
    /// Start the login apps (XDG autostart) once the tray is up, once a
    /// session. `nova` asks for it when it starts novad without systemd,
    /// whose own autostart does it otherwise.
    #[arg(long)]
    autostart: bool,
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
    /// The modules kept for novad's life, reached without the settings
    /// lock: a reload holds it while modules start, which may take seconds.
    kept: modules::Kept,
}

impl Daemon {
    fn status(&self) -> Status {
        let disabled = UiStatus { state: UiState::Disabled, pid: None, pss_kib: None, restarts: 0, last_crash: None };
        Status {
            daemon: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            uptime_secs: self.started.elapsed().as_secs(),
            pss_kib: procfs::pss_kib(Path::new(procfs::PROC), std::process::id()),
            quickshell: self.ui.as_ref().and_then(|ui| ui.quickshell_version()),
            ui: self.ui.as_ref().map_or(disabled, |ui| ui.status()),
            features: lock(&self.settings).features.clone(),
            log_dir: self.logger.dir().display().to_string(),
            desktop: self.compositor.as_ref().and_then(|compositor| compositor.desktop()),
        }
    }

    /// Whether the feature `id` is ready.
    fn feature_ready(&self, id: &str) -> bool {
        features::is_ready(&lock(&self.settings).features, id)
    }

    /// Reads the settings files again and applies what changed.
    fn reload(&self) {
        let settings = &mut lock(&self.settings);
        settings.reload(&self.sources, &self.hub, &self.logger);
        self.follow_features(settings);
    }

    /// Runs the UI while some feature that draws is ready.
    fn follow_features(&self, settings: &Settings) {
        if let Some(ui) = &self.ui {
            ui.want(features::needs_ui(&self.sources.manifests, &settings.features), &self.logger);
        }
    }

    /// The lock screen, kept whatever the settings.
    fn session_lock(&self) -> Arc<session_lock::SessionLock> {
        Arc::clone(&self.kept.session_lock)
    }

    /// The module `M`, or the error that its feature isn't ready. Taken out
    /// of the settings lock: acting may take a while, and a reload may stop
    /// it meanwhile (it then says its feature isn't ready).
    fn module<M: modules::Module>(&self) -> Result<Arc<M>, String>
    where
        modules::Running: modules::Has<M>,
    {
        lock(&self.settings).modules.get::<M>().ok_or_else(|| features::not_ready(M::ID))
    }

    /// Starts watching the settings files, reloading when they change.
    fn watch_settings(self: &Arc<Self>) -> Option<inotify::Stopper> {
        let daemon = Arc::clone(self);
        let files = [vec![self.sources.config.clone()], self.sources.selection.clone()].concat();
        settings::watch(files, Arc::clone(&self.logger), move || daemon.reload())
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
    // systemd's, for novad alone: neither the UI nor the apps novad starts
    // may answer it.
    let notify_socket = std::env::var_os(notify::SOCKET_VAR);
    // SAFETY: no other thread runs yet to read the environment meanwhile.
    unsafe { std::env::remove_var(notify::SOCKET_VAR) };
    let (listener, daemon, _watching) = start(args, socket)?;
    install_panic_hook(Arc::clone(&daemon.logger));
    // Up: the socket listens and the tray's name is claimed, so login apps
    // may start (nova.service is Type=notify).
    if let Err(e) = notify::ready(notify_socket.as_deref()) {
        daemon.logger.warn(LOG_SOURCE, &format!("can't tell systemd novad is ready: {e}"));
    }
    server::serve(listener, daemon, &AtomicBool::new(false));
    Ok(())
}

/// Starts novad on background threads in this process and returns once its
/// socket accepts connections. For tests of novad's clients; it stops when
/// the returned handle is dropped.
pub fn spawn(args: Args, socket: PathBuf) -> Result<InProcess, Box<dyn std::error::Error>> {
    let (listener, daemon, watching) = start(args, socket)?;
    Ok(InProcess::serve(listener, daemon, watching))
}

/// A novad serving on a thread of this process ([`spawn`], novad's own
/// tests). Dropping it stops the accept loop and waits for it to end, and
/// stops the UI, the feature services and the settings watcher, so tests
/// sharing one process don't pile up threads, children and inotify
/// instances. Connections still open end when their clients hang up.
pub struct InProcess {
    daemon: Arc<Daemon>,
    stopped: Arc<AtomicBool>,
    serving: Option<std::thread::JoinHandle<()>>,
    watching: Option<inotify::Stopper>,
}

impl InProcess {
    fn serve(
        listener: std::os::unix::net::UnixListener,
        daemon: Arc<Daemon>,
        watching: Option<inotify::Stopper>,
    ) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let serving = {
            let (daemon, stopped) = (Arc::clone(&daemon), Arc::clone(&stopped));
            std::thread::spawn(move || server::serve(listener, daemon, &stopped))
        };
        Self { daemon, stopped, serving: Some(serving), watching }
    }
}

impl Drop for InProcess {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        // Wakes the accept loop, which then sees the stop.
        let _ = std::os::unix::net::UnixStream::connect(&self.daemon.socket);
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
        // The watcher first: a reload after the services stop would start
        // them again.
        if let Some(watching) = &self.watching {
            watching.stop();
        }
        if let Some(ui) = &self.daemon.ui {
            ui.stop();
        }
        lock(&self.daemon.settings).stop_modules();
    }
}

/// A started novad before it serves: its bound socket, itself, and its
/// settings watcher.
type Started = (std::os::unix::net::UnixListener, Arc<Daemon>, Option<inotify::Stopper>);

fn start(args: Args, socket: PathBuf) -> Result<Started, Box<dyn std::error::Error>> {
    // The socket first: a second daemon is refused before it can rotate the
    // running one's log.
    let listener = server::bind(&socket)?;
    let hub = Arc::new(Hub::default());
    let logger = Arc::new(Logger::open(&args.log_dir, log_level(), std::io::stderr().is_terminal(), Arc::clone(&hub))?);
    logger.info(LOG_SOURCE, &format!("novad {} starting (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));

    let path_var = session_path();
    let runtime_dir = socket.parent().unwrap_or(Path::new(".")).to_path_buf();
    let icons = Arc::new(icons::Icons::for_session());
    logger.info(LOG_SOURCE, &format!("icons from the {} icon theme", icons.theme()));
    // How Quickshell runs, for the UI and for modules with windows of their own.
    let ui_settings = UiSettings {
        quickshell: args.quickshell.clone().unwrap_or_else(|| supervisor::default_quickshell(&path_var)),
        ui_dir: args.ui.clone(),
        socket: socket.clone(),
        path_var: path_var.clone(),
        wayland_socket: wayland_socket(),
        egl_vendor: egl::nvidia_only_vendor(Path::new("/"), |name| std::env::var_os(name)),
        render_loop: render_loop::for_ui(|name| std::env::var_os(name)),
        icon_theme: icons.theme().to_string(),
    };
    let env = modules::Env {
        ui: (!args.no_ui).then_some(&ui_settings),
        args: &args,
        runtime_dir: &runtime_dir,
        path_var: &path_var,
        icons: &icons,
        home: nova_paths::home(),
    };
    let modules = modules::Sources::system(&env);
    let sources = Sources {
        config: args.config.clone(),
        selection: args.selection.clone().map_or_else(|| nova_paths::config_files("features.toml"), |path| vec![path]),
        themes_dir: args.themes_dir.clone(),
        manifests: nova_features::load_all(&args.features_dir)?,
        path_var: path_var.clone(),
        modules,
        icons,
    };
    // Before the modules: the wallpaper follows its outputs.
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
    let settings = Settings::load(&sources, &hub, &logger, compositor.clone());
    if args.autostart {
        launcher::autostart::start(&socket.with_file_name(autostart_marker()), path_var.clone(), Arc::clone(&logger));
    }

    logger.info(LOG_SOURCE, &format!("listening on {}", socket.display()));

    let ui = (!args.no_ui).then(|| {
        if ui_settings.wayland_socket.is_none() {
            logger.warn(
                LOG_SOURCE,
                "WAYLAND_DISPLAY isn't set, so the UI can't open a window. If systemd started novad, \
                 run `nova` from your compositor's autostart so it can pass the session on.",
            );
        }
        let wanted = features::needs_ui(&sources.manifests, &settings.features);
        Supervisor::start(ui_settings, wanted, Arc::clone(&logger))
    });

    let daemon = Arc::new(Daemon {
        started: Instant::now(),
        hub,
        logger,
        ui,
        socket,
        sources,
        kept: settings.modules.kept(),
        settings: Mutex::new(settings),
        compositor,
    });
    // Watched for as long as novad runs.
    let watching = daemon.watch_settings();
    // Not for a daemon without its UI (tests, a UI run by hand): logind's
    // locks and the sleep inhibitor are the whole machine's.
    if daemon.ui.is_some() {
        logind::follow(&daemon);
    }
    Ok((listener, daemon, watching))
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

/// `PATH` plus the folders packages put programs in, even ones not there
/// yet (a first package installed later makes them):
/// systemd user services don't get the login shell's PATH, so the user's
/// Nix profile, NixOS's system packages, Flatpak's exports or `~/.local/bin`
/// (pip, cargo) may be missing. Apps the launcher starts get it too.
pub(crate) fn session_path() -> String {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let home = nova_paths::home();
    let extra = home
        .iter()
        .map(|home| home.join(".local/bin"))
        .chain(package_prefixes().into_iter().map(|prefix| prefix.join("bin")));
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Makes the app `app` (a desktop file id) the default for `mime` in this
/// user's own `mimeapps.list` under `config_home`, keeping the rest of it,
/// as Nova Files' "Open with" does. For `nova setup`. Returns the file
/// written.
pub fn set_default_app(config_home: &Path, mime: &str, app: &str) -> Result<PathBuf, String> {
    let places = apps::associations::Places::new(Some(config_home.to_owned()), &[], &[], &apps::current_desktops());
    apps::associations::set_default(&places, mime, app)
}

pub use portal_config::prefer_nova_file_chooser;

/// The spec's default for an unset `XDG_DATA_DIRS`.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// The XDG data folders (the user's first), in the order they're searched
/// for apps and icons, plus the folders packages install into, which a
/// systemd user service's `XDG_DATA_DIRS` often lacks.
pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(nova_paths::data_home());
    let system = nova_paths::path_var("XDG_DATA_DIRS").unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    dirs.extend(std::env::split_paths(&system));
    // Where packages install, last so the session's own order wins.
    dirs.extend(package_prefixes().into_iter().map(|prefix| prefix.join("share")));
    let mut seen = std::collections::BTreeSet::new();
    dirs.retain(|dir| dir.is_absolute() && seen.insert(dir.clone()));
    dirs
}

/// Where package managers install, each with `bin/` and `share/` below it,
/// in the order they're searched after the session's own: the user's Nix
/// profiles, the system's (NixOS), Flatpak's exports, then the usual.
pub(crate) fn package_prefixes() -> Vec<PathBuf> {
    let home = nova_paths::home();
    let mut prefixes = Vec::new();
    prefixes
        .extend(home.iter().flat_map(|home| [home.join(".nix-profile"), home.join(".local/share/flatpak/exports")]));
    prefixes.extend(
        std::env::var_os("USER")
            .filter(|user| !user.is_empty())
            .map(|user| Path::new("/etc/profiles/per-user").join(user)),
    );
    prefixes.extend(
        [
            "/nix/var/nix/profiles/default",
            "/run/current-system/sw",
            "/var/lib/flatpak/exports",
            "/usr/local",
            "/usr",
            "/",
        ]
        .map(PathBuf::from),
    );
    prefixes
}

fn autostart_marker() -> String {
    match std::env::var("XDG_SESSION_ID") {
        Ok(session) if !session.is_empty() && session.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("{AUTOSTART_MARKER}-{session}")
        }
        _ => AUTOSTART_MARKER.to_string(),
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
