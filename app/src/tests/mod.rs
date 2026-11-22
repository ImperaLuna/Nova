//! Component tests: novad driven through its real socket, with a real logger,
//! supervisor, settings files and feature manifests. The stand-ins are the
//! UI process in the crash tests, a shell script, since Quickshell needs a
//! compositor, and compositor IPC servers replaying recorded niri and
//! Hyprland sessions.

use super::*;
use nova_carrier::log::{LogEntry, UI_LOG_SOURCE};
use nova_daemon_info::FeatureStatus;
use nova_icons::Icons;
use nova_session_bus::Bus;
use nova_test_support::{TempDir, wait_until, write_program};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Lines, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// A novad in this process for one test, in a folder of its own. Dropping
/// it stops everything it started (see [`InProcess`]), then removes the
/// folder.
struct TestDaemon {
    // Before `dir`: fields drop in order.
    _running: InProcess,
    dir: TempDir,
    socket: PathBuf,
    daemon: Arc<Daemon>,
}

/// How a test's novad runs `program` as its UI, with `quickshell` as its
/// Quickshell. Tests get no Wayland session, so the supervisor never takes
/// the compositor for gone.
fn ui_settings(program: Program, quickshell: PathBuf) -> UiSettings {
    UiSettings {
        program,
        quickshell,
        ui_dir: PathBuf::from("ui"),
        socket: PathBuf::from("unused.sock"),
        path_var: std::env::var("PATH").unwrap_or_default(),
        wayland_socket: None,
        egl_vendor: None,
        render_loop: None,
        icon_theme: "hicolor".into(),
    }
}

/// `features.toml` with features off. The features with a watcher of their own, off unless a test is about
/// them: each watcher is an inotify instance, and the tests running at once
/// would pass the usual limit of 128.
const WATCHERS_OFF: &str = "disabled = [\"ai-usage\", \"launcher\"]\n";
const LAUNCHER_OFF: &str = "disabled = [\"launcher\"]\n";
const AI_USAGE_OFF: &str = "disabled = [\"ai-usage\"]\n";

impl TestDaemon {
    /// With the `ai-usage` and `launcher` features off.
    fn start(name: &str, ui: Option<UiSettings>) -> Self {
        Self::open(name, ui, |dir| std::fs::write(dir.join("config/features.toml"), WATCHERS_OFF).unwrap())
    }

    /// With the `ai-usage` feature on, its files under `<dir>/ai-usage`, and
    /// the `launcher` off.
    fn with_ai_usage(name: &str, prepare: impl FnOnce(&Path)) -> Self {
        Self::open(name, None, |dir| {
            std::fs::write(dir.join("config/features.toml"), LAUNCHER_OFF).unwrap();
            prepare(&dir.join("ai-usage"));
        })
    }

    /// With the `launcher` feature on, its apps under
    /// `<dir>/launcher/share`, and `ai-usage` off.
    fn with_launcher(name: &str) -> Self {
        Self::open(name, None, |dir| std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap())
    }

    /// Like [`TestDaemon::with_launcher`], following the compositor `connect`
    /// sets up in the test's folder.
    fn with_launcher_following(name: &str, connect: impl FnOnce(&Path) -> Box<dyn nova_compositor::Backend>) -> Self {
        Self::open_following(
            name,
            None,
            |dir| std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap(),
            |dir| Some(connect(dir)),
        )
    }

    /// Following the compositor `connect` sets up in the test's folder, with
    /// the `ai-usage` and `launcher` features off.
    pub fn with_compositor(name: &str, connect: impl FnOnce(&Path) -> Box<dyn nova_compositor::Backend>) -> Self {
        Self::open_following(
            name,
            None,
            |dir| std::fs::write(dir.join("config/features.toml"), WATCHERS_OFF).unwrap(),
            |dir| Some(connect(dir)),
        )
    }

    /// Settings files go in `<dir>/config`, which `prepare` may fill or remove.
    fn open(name: &str, ui: Option<UiSettings>, prepare: impl FnOnce(&Path)) -> Self {
        Self::open_following(name, ui, prepare, |_| None)
    }

    /// Like [`TestDaemon::open`], following the compositor `compositor` sets up.
    fn open_following(
        name: &str,
        ui: Option<UiSettings>,
        prepare: impl FnOnce(&Path),
        compositor: impl FnOnce(&Path) -> Option<Box<dyn nova_compositor::Backend>>,
    ) -> Self {
        let temp = TempDir::new(name);
        let dir = temp.path().to_owned();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::create_dir_all(dir.join("ai-usage")).unwrap();
        std::fs::create_dir_all(dir.join("launcher/share/applications")).unwrap();
        prepare(&dir);
        // The `upower` and `bluetoothctl` commands `peripherals` and
        // `bluetooth` check for, which a test machine may lack; novad never
        // runs them.
        std::fs::create_dir_all(dir.join("fake-bin")).unwrap();
        write_program(&dir.join("fake-bin"), "upower", "exit 0");
        // And `nmcli`, which `network` checks for.
        write_program(&dir.join("fake-bin"), "nmcli", "exit 0");
        write_program(&dir.join("fake-bin"), "bluetoothctl", "exit 0");
        let host_path = std::env::var("PATH").unwrap_or_default();
        let socket = dir.join("novad.sock");
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let listener = nova_socket::bind(&socket).unwrap();
        let icons = test_icons(&dir);
        let sources = Sources {
            config: dir.join("config/config.toml"),
            selection: vec![dir.join("config/features.toml")],
            themes_dir: nova_theme::default_dir(),
            manifests: nova_features::load_all(&repo_features()).unwrap(),
            path_var: format!("{host_path}:{}", dir.join("fake-bin").display()),
            modules: test_modules(&dir, &icons),
            icons,
        };
        let compositor = compositor(&dir)
            .map(|backend| nova_compositor::Compositor::start(backend, Arc::clone(&hub), Arc::clone(&logger)));
        let settings = Settings::load(&sources, &hub, &logger, compositor.clone());
        let wanted = features::needs_ui(&sources.manifests, &settings.features);
        let ui = ui.map(|ui_settings| Supervisor::start(ui_settings, wanted, Arc::clone(&logger)));
        let daemon = Arc::new(Daemon {
            started: Instant::now(),
            hub,
            logger,
            ui,
            socket: socket.clone(),
            sources,
            kept: settings.modules.kept(),
            settings: Mutex::new(settings),
            compositor,
            router: crate::requests::router(),
        });
        let running = InProcess::serve(listener, Arc::clone(&daemon), daemon.watch_settings());
        Self { _running: running, dir: temp, socket, daemon }
    }

    fn config_file(&self) -> PathBuf {
        self.daemon.sources.config.clone()
    }

    fn features_file(&self) -> PathBuf {
        self.daemon.sources.selection[0].clone()
    }

    /// Connects and reads the hello.
    fn connect(&self) -> Client {
        let stream = UnixStream::connect(&self.socket).unwrap();
        let writer = stream.try_clone().unwrap();
        let mut client = Client { writer, lines: BufReader::new(stream).lines(), early: Default::default() };
        assert_eq!(client.next()["event"], "hello");
        client
    }
}

struct Client {
    writer: UnixStream,
    lines: Lines<BufReader<UnixStream>>,
    /// Events that arrived while `request` waited for its reply.
    early: std::collections::VecDeque<Value>,
}

impl Client {
    /// The data of the next `name` event, skipping anything else; fails
    /// after a while.
    fn next_event(&mut self, name: &str) -> Value {
        self.writer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        loop {
            let message = match self.early.pop_front() {
                Some(event) => event,
                None => {
                    let line = self.lines.next().expect("connection closed").expect("no event in time");
                    serde_json::from_str(&line).unwrap()
                }
            };
            if message["event"] == name {
                return message["data"].clone();
            }
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.writer, "{message}").unwrap();
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.writer, "{line}").unwrap();
    }

    fn next(&mut self) -> Value {
        if let Some(event) = self.early.pop_front() {
            return event;
        }
        serde_json::from_str(&self.lines.next().unwrap().unwrap()).unwrap()
    }

    fn request(&mut self, id: u64, cmd: &str, args: Option<Value>) -> Value {
        let mut message = json!({"v": 2, "id": id, "cmd": cmd});
        if let Some(args) = args {
            message["args"] = args;
        }
        self.send(&message);
        // Events published meanwhile (ai_usage's worker) may come first;
        // they're kept for `next`.
        loop {
            let message: Value = serde_json::from_str(&self.lines.next().unwrap().unwrap()).unwrap();
            if message.get("event").is_none() {
                assert_eq!(message["id"], id, "expected the reply to {id}, got {message}");
                return message;
            }
            self.early.push_back(message);
        }
    }
}

fn read_entries(path: &Path) -> Vec<LogEntry> {
    std::fs::read_to_string(path).unwrap().lines().filter_map(LogEntry::parse_line).collect()
}

/// Apps come from `<dir>/launcher/share` only, and start forked twice,
/// never as services of the desktop's systemd.
/// The modules' sources for a test novad in `dir`: apps under
/// `<dir>/launcher/share`, and no bus, UPower, NetworkManager or BlueZ
/// unless a test starts one there.
fn test_modules(dir: &Path, icons: &Arc<Icons>) -> modules::Sources {
    modules::Sources {
        apps: nova_apps::Sources { data_dirs: vec![dir.join("launcher/share")], icons: Arc::clone(icons) },
        ai_usage: ai_usage_paths(&dir.join("ai-usage")),
        resources: nova_resources::Sources::system(""),
        launcher: nova_launcher::Paths {
            state_file: dir.join("launcher/launcher.json"),
            runner: nova_launcher::Runner::Direct,
        },
        // No bus for the tray and notifications unless the test starts
        // one there (`nova_session_bus::TestBus`).
        tray: nova_tray::Sources {
            bus: nova_session_bus::Bus::Address(format!("unix:path={}", dir.join("bus").display())),
            state_file: dir.join("tray.json"),
            proc_root: nova_process::procfs::PROC.into(),
        },
        notifications: nova_notifications::Sources {
            bus: Bus::Address(format!("unix:path={}", dir.join("bus").display())),
            state_file: dir.join("notifications.json"),
            images: dir.join("notifications"),
            icons: Arc::clone(icons),
        },
        // No bus: its own tests give it one.
        nova_files: crate::nova_files::Sources {
            bus: Bus::Address(format!("unix:path={}", dir.join("no-bus").display())),
            data_dirs: vec![],
            home: None,
        },
        // No bus either: the media's own tests give it one.
        media: nova_media::Sources {
            bus: Bus::Address(format!("unix:path={}", dir.join("no-bus").display())),
            covers: dir.join("media"),
            proc: dir.join("proc"),
            process_names: crate::media::process_names,
        },
        // No UPower, hidraw nodes, HeadsetControl or USB devices
        // either: the peripherals' own tests give it theirs.
        peripherals: nova_peripherals::Sources {
            upower: Bus::Address(format!("unix:path={}", dir.join("no-bus").display())),
            sys_root: dir.join("sys"),
            dev: dir.join("dev"),
            open_hidraw: Arc::new(|_| Err(std::io::ErrorKind::NotFound.into())),
            path_var: dir.join("no-programs").display().to_string(),
            usb_devices: dir.join("usb"),
        },
        // No NetworkManager either: the network's own tests give it one.
        network: nova_network::Sources {
            bus: Bus::Address(format!("unix:path={}", dir.join("no-bus").display())),
            sys_root: dir.join("sys"),
            state_dir: dir.join("state"),
        },
        // No BlueZ either, and never the system's: a test novad must not
        // become the machine's pairing agent. Its own tests give it one.
        bluetooth: nova_bluetooth::Sources {
            bus: Bus::Address(format!("unix:path={}", dir.join("no-bus").display())),
            rfkill: nova_bluetooth::Rfkill {
                class: dir.join("sys/class/rfkill"),
                device: dir.join("dev/rfkill"),
                adapters: dir.join("sys/class/bluetooth"),
            },
            connect_limit: nova_bluetooth::CONNECT_LIMIT,
            audio_wait: nova_bluetooth::AUDIO_WAIT,
        },
        x11_tray: nova_session_bus::Bus::Address(format!("unix:path={}", dir.join("bus").display())),
        session_lock: dir.to_owned(),
        screenshots: dir.to_owned(),
        recordings: (),
        wallpaper: wallpaper::Sources { state_dir: dir.to_owned(), home: None },
    }
}

/// Icons come from `<dir>/launcher/share` only, in hicolor.
fn test_icons(dir: &Path) -> Arc<Icons> {
    Arc::new(Icons::new(nova_icons::HICOLOR.into(), &[dir.join("launcher/share")], None))
}

fn repo_features() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../features")
}

/// The usage endpoints are `claude_usage_url` and `codex_usage_url`; nothing
/// listens there unless a test starts a fake one.
fn ai_usage_paths(root: &Path) -> nova_ai_usage::Paths {
    nova_ai_usage::Paths {
        codex_home: Some(root.join("codex")),
        state_file: root.join("state/ai-usage.json"),
        claude_credentials: Some(root.join("claude/.credentials.json")),
        // Missing, so the keyring is never asked.
        claude_desktop: Some(nova_ai_usage::claude_desktop_in(&root.join("claude-desktop"))),
        claude_usage_url: std::fs::read_to_string(root.join("claude/url")).unwrap_or("http://127.0.0.1:9/".into()),
        codex_usage_url: std::fs::read_to_string(root.join("codex-url")).unwrap_or("http://127.0.0.1:9/".into()),
    }
}

mod apps;
mod basics;
mod desktop;
mod fake_compositor;
mod live_config;
mod services;
mod usage;
mod wire;

use fake_compositor::{FakeCompositor, focused_output, follow_to_the_end, workspace_id};
