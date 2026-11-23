mod ai_usage;
mod bluetooth;
pub mod checks;
mod config;
mod features;
mod launcher;
mod media;
mod modules;
mod network;
mod notifications;
mod nova_files;
mod peripherals;
mod power;
mod recording;
mod requests;
mod resources;
mod screenshot;
mod session_lock;
mod settings;
mod sound;
mod supervisor;
mod topics;
mod tray;
mod wallpaper;
mod x11_tray;

use nova_base::lock;
use nova_carrier::log::LogLevel;
use nova_daemon_info::{Status, UiProgram, UiState, UiStatus};
use nova_hub::Hub;
use nova_log::Logger;
use nova_process::{notify, procfs};
use settings::{Settings, Sources};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use supervisor::{Supervisor, UiSettings};

pub const PROCESS_NAME: &str = "Nova";
const LOG_LEVEL_VAR: &str = "NOVA_LOG";
const LOG_SOURCE: &str = "novad";
const AUTOSTART_MARKER: &str = "autostarted";

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value_os_t = nova_features::default_dir())]
    features_dir: PathBuf,
    #[arg(long)]
    selection: Option<PathBuf>,
    #[arg(long, default_value_os_t = nova_theme::default_dir())]
    themes_dir: PathBuf,
    #[arg(long, default_value_os_t = nova_paths::config_dir().join("config.toml"))]
    config: PathBuf,
    #[arg(long)]
    gpui_ui: Option<PathBuf>,
    #[arg(long)]
    no_ui: bool,
    #[arg(long, default_value_os_t = nova_paths::state_dir())]
    log_dir: PathBuf,
    #[arg(long, default_value_os_t = nova_paths::state_dir())]
    state_dir: PathBuf,
    #[arg(long)]
    codex_home: Option<PathBuf>,
    #[arg(long)]
    claude_credentials: Option<PathBuf>,
    #[arg(long)]
    claude_desktop_config: Option<PathBuf>,
    #[arg(long)]
    autostart: bool,
}

pub struct Daemon {
    started: Instant,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    ui: Option<Arc<Supervisor>>,
    socket: PathBuf,
    sources: Sources,
    settings: Mutex<Settings>,
    compositor: Option<Arc<nova_compositor::Compositor>>,
    kept: modules::Kept,
    router: nova_socket::Router<Daemon>,
}

impl nova_socket::Service for Daemon {
    type Hello = nova_daemon_info::Event;

    fn hub(&self) -> &Hub {
        &self.hub
    }

    fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    fn router(&self) -> &nova_socket::Router<Self> {
        &self.router
    }

    fn hello(&self) -> Self::Hello {
        nova_daemon_info::Event::Hello { daemon: env!("CARGO_PKG_VERSION").into() }
    }

    fn client_gone(&self, client: nova_hub::ClientId) {
        lock(&self.settings).modules.client_gone(client);
    }
}

impl Daemon {
    fn status(&self) -> Status {
        let disabled = UiStatus {
            program: UiProgram::Gpui,
            state: UiState::Disabled,
            pid: None,
            pss_kib: None,
            restarts: 0,
            last_crash: None,
            since: None,
            problem: None,
        };
        Status {
            daemon: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            uptime_secs: self.started.elapsed().as_secs(),
            pss_kib: procfs::pss_kib(Path::new(procfs::PROC), std::process::id()),
            quickshell: None,
            ui: self.ui.as_ref().map_or(disabled, |ui| ui.status()),
            features: lock(&self.settings).features.clone(),
            log_dir: self.logger.dir().display().to_string(),
            desktop: self.compositor.as_ref().and_then(|compositor| compositor.desktop()),
        }
    }

    fn feature_ready(&self, id: &str) -> bool {
        features::is_ready(&lock(&self.settings).features, id)
    }

    fn reload(&self) {
        let settings = &mut lock(&self.settings);
        settings.reload(&self.sources, &self.hub, &self.logger);
        self.follow_features(settings);
    }

    fn follow_features(&self, settings: &Settings) {
        if let Some(ui) = &self.ui {
            ui.want(features::needs_ui(&self.sources.manifests, &settings.features), &self.logger);
        }
    }

    fn session_lock(&self) -> Arc<session_lock::SessionLock> {
        Arc::clone(&self.kept.session_lock)
    }

    fn module<M: nova_modules::Module>(&self) -> Result<Arc<M>, String>
    where
        modules::Running: nova_modules::Has<M>,
    {
        lock(&self.settings).modules.get::<M>().ok_or_else(|| nova_modules::not_ready(M::ID))
    }

    fn watch_settings(self: &Arc<Self>) -> Option<nova_inotify::Stopper> {
        let daemon = Arc::clone(self);
        let files = [vec![self.sources.config.clone()], self.sources.selection.clone()].concat();
        nova_settings::watch(files, Arc::clone(&self.logger), move || daemon.reload())
    }

    fn quit(&self) -> ! {
        self.logger.info(LOG_SOURCE, "quit requested, stopping");
        if let Some(ui) = &self.ui {
            ui.stop();
        }
        let _ = std::fs::remove_file(&self.socket);
        std::process::exit(0)
    }
}

pub fn run(args: Args, socket: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    set_process_name(PROCESS_NAME)?;
    let notify_socket = std::env::var_os(notify::SOCKET_VAR);
    // SAFETY: no other thread runs yet to read the environment meanwhile.
    unsafe { std::env::remove_var(notify::SOCKET_VAR) };
    let (listener, daemon, _watching) = start(args, socket)?;
    install_panic_hook(Arc::clone(&daemon.logger));
    if let Err(e) = notify::ready(notify_socket.as_deref()) {
        daemon.logger.warn(LOG_SOURCE, &format!("can't tell systemd novad is ready: {e}"));
    }
    nova_socket::serve(listener, daemon, &AtomicBool::new(false));
    Ok(())
}

pub fn spawn(args: Args, socket: PathBuf) -> Result<InProcess, Box<dyn std::error::Error>> {
    let (listener, daemon, watching) = start(args, socket)?;
    Ok(InProcess::serve(listener, daemon, watching))
}

pub struct InProcess {
    daemon: Arc<Daemon>,
    stopped: Arc<AtomicBool>,
    serving: Option<std::thread::JoinHandle<()>>,
    watching: Option<nova_inotify::Stopper>,
}

impl InProcess {
    fn serve(
        listener: std::os::unix::net::UnixListener,
        daemon: Arc<Daemon>,
        watching: Option<nova_inotify::Stopper>,
    ) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let serving = {
            let (daemon, stopped) = (Arc::clone(&daemon), Arc::clone(&stopped));
            std::thread::spawn(move || nova_socket::serve(listener, daemon, &stopped))
        };
        Self { daemon, stopped, serving: Some(serving), watching }
    }
}

impl Drop for InProcess {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = std::os::unix::net::UnixStream::connect(&self.daemon.socket);
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
        if let Some(watching) = &self.watching {
            watching.stop();
        }
        if let Some(ui) = &self.daemon.ui {
            ui.stop();
        }
        lock(&self.daemon.settings).stop_modules();
    }
}

type Started = (std::os::unix::net::UnixListener, Arc<Daemon>, Option<nova_inotify::Stopper>);

fn ui_settings(args: &Args, socket: &Path, path_var: &str) -> UiSettings {
    UiSettings {
        program: args.gpui_ui.clone(),
        socket: socket.to_path_buf(),
        path_var: path_var.to_owned(),
        wayland_socket: wayland_socket(),
    }
}

fn follow_compositor(hub: &Arc<Hub>, logger: &Arc<Logger>) -> Option<Arc<nova_compositor::Compositor>> {
    let Some(backend) = nova_compositor::detect(|name| std::env::var_os(name)) else {
        logger
            .warn(LOG_SOURCE, "neither NIRI_SOCKET nor HYPRLAND_INSTANCE_SIGNATURE is set, so no workspaces will show");
        return None;
    };
    Some(nova_compositor::Compositor::start(backend, Arc::clone(hub), Arc::clone(logger)))
}

fn start(args: Args, socket: PathBuf) -> Result<Started, Box<dyn std::error::Error>> {
    let listener = nova_socket::bind(&socket)?;
    let hub = Arc::new(Hub::default());
    let logger = Arc::new(Logger::open(&args.log_dir, log_level(), std::io::stderr().is_terminal(), Arc::clone(&hub))?);
    logger.info(LOG_SOURCE, &format!("novad {} starting (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));

    let path_var = session_path();
    let runtime_dir = socket.parent().unwrap_or(Path::new(".")).to_path_buf();
    let icons = Arc::new(nova_icons::Icons::for_session());
    logger.info(LOG_SOURCE, &format!("icons from the {} icon theme", icons.theme()));
    let env = modules::Env {
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
    let compositor = follow_compositor(&hub, &logger);
    let settings = Settings::load(&sources, &hub, &logger, compositor.clone());
    let ui_settings = ui_settings(&args, &socket, &path_var);
    if args.autostart {
        nova_launcher::autostart::start(
            &socket.with_file_name(autostart_marker()),
            path_var.clone(),
            Arc::clone(&logger),
        );
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
        router: requests::router(),
    });
    let watching = daemon.watch_settings();
    if daemon.ui.is_some() {
        power::follow_logind(&daemon);
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

fn install_panic_hook(logger: Arc<Logger>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = logger.try_save_crash(&format!("novad panicked: {info}")) {
            eprintln!("novad: crash log saved to {}", path.display());
        }
        default_hook(info);
    }));
}

pub(crate) fn session_path() -> String {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let home = nova_paths::home();
    let extra = home
        .iter()
        .map(|home| home.join(".local/bin"))
        .chain(nova_paths::package_prefixes().into_iter().map(|prefix| prefix.join("bin")));
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

fn autostart_marker() -> String {
    match std::env::var("XDG_SESSION_ID") {
        Ok(session) if !session.is_empty() && session.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("{AUTOSTART_MARKER}-{session}")
        }
        _ => AUTOSTART_MARKER.to_string(),
    }
}

fn set_process_name(name: &str) -> std::io::Result<()> {
    let name = std::ffi::CString::new(name).map_err(std::io::Error::other)?;
    // SAFETY: PR_SET_NAME reads a NUL-terminated string, which `name` is
    // for the whole call; the kernel copies at most 16 bytes of it.
    if unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr()) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn wayland_socket() -> Option<PathBuf> {
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").filter(|value| !value.is_empty())?);
    if display.is_absolute() {
        return Some(display);
    }
    Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(display))
}

pub use nova_ai_usage::limits_from_statusline as claude_limits_from_statusline;

#[cfg(test)]
mod tests;
