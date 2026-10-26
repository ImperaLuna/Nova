//! `nova doctor`: one line per check, ok, warn or fail, and a non-zero exit
//! when anything fails. A warning is something Nova runs with but works
//! worse for, like no compositor it follows.
//!
//! Facts are gathered first (`Facts::gather`), then each check turns its
//! facts into a line, so the checks are tested on made-up facts.

use crate::client::Client;
use crate::host::{Distro, Host};
use crate::lifecycle::RESTART_COMMAND;
use crate::login_apps::{self, AUTOSTART_DIR, StartupCommand};
use nova_features::Gpu;
use nova_protocol::{Event, FeatureState, FeatureStatus, Topic, TrayItem};
use nova_quickshell::Range;
use novad::checks;
use std::fmt;
use std::path::Path;

/// Qt's render loop variable, and the value that keeps the UI's memory down
/// on NVIDIA's driver.
const RENDER_LOOP_VAR: &str = "QSG_RENDER_LOOP";
const RENDER_LOOP_FOR_NVIDIA: &str = "basic";
const SETUP_COMMAND: &str = "nova setup";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, PartialEq)]
pub struct Line {
    level: Level,
    check: &'static str,
    detail: String,
}

impl Line {
    fn new(level: Level, check: &'static str, detail: impl Into<String>) -> Self {
        Self { level, check, detail: detail.into() }
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        };
        write!(f, "{level:<6}{:<12}{}", self.check, crate::status::printable(&self.detail))
    }
}

/// Whether novad answered on its socket.
#[derive(Debug)]
pub enum Daemon {
    Running {
        version: String,
        /// The apps in the tray, when its feature is on.
        tray: Vec<TrayItem>,
    },
    /// No socket, or nothing listening on it.
    NotRunning,
    /// Something listens but doesn't answer like novad.
    NoAnswer(String),
}

/// Everything the checks look at.
pub struct Facts {
    pub host: Host,
    pub compositor: Option<&'static str>,
    pub quickshell: Result<String, String>,
    pub daemon: Daemon,
    /// config.toml's theme problems, or why it doesn't parse.
    pub config: Result<Vec<String>, String>,
    /// Each feature's state with this session's `PATH`, or why they couldn't
    /// be worked out.
    pub features: Result<Vec<FeatureState>, String>,
    pub render_loop: Option<String>,
    /// What the compositor starts at login.
    pub startup: Vec<StartupCommand>,
}

impl Facts {
    pub fn gather(socket: &Path) -> Self {
        let path_var = checks::session_path();
        let config_dir = nova_paths::config_dir();
        let quickshell = checks::default_quickshell(&path_var);
        Self {
            host: Host::detect(Path::new("/")),
            compositor: checks::compositor(|name| std::env::var_os(name)),
            quickshell: checks::read_quickshell_version(&quickshell, &path_var),
            daemon: reach(socket),
            config: checks::read_config(&config_dir.join("config.toml"), &checks::default_themes_dir()),
            features: features(&nova_features::default_dir(), &config_dir.join("features.toml"), &path_var),
            render_loop: std::env::var(RENDER_LOOP_VAR).ok(),
            startup: config_dir.parent().map(login_apps::read).unwrap_or_default(),
        }
    }
}

/// Tells "not running" from "stuck" by how the one connection fails, so the
/// doctor never makes a second connection that could wait on a stuck novad.
fn reach(socket: &Path) -> Daemon {
    use std::io::ErrorKind;
    match std::os::unix::net::UnixStream::connect(socket) {
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => Daemon::NotRunning,
        Err(e) => Daemon::NoAnswer(format!("can't reach novad at {}: {e}", socket.display())),
        Ok(stream) => match Client::over(stream) {
            Ok(mut client) => {
                // Only for a warning: a novad too old to answer is still running.
                let events = client.current(vec![Topic::Tray]).unwrap_or_default();
                let tray = events
                    .into_iter()
                    .find_map(|event| match event {
                        Event::Tray(tray) => Some(tray.items),
                        _ => None,
                    })
                    .unwrap_or_default();
                Daemon::Running { version: client.daemon_version, tray }
            }
            Err(problem) => Daemon::NoAnswer(problem),
        },
    }
}

fn features(manifests_dir: &Path, selection: &Path, path_var: &str) -> Result<Vec<FeatureState>, String> {
    let manifests = nova_features::load_all(manifests_dir).map_err(|e| e.to_string())?;
    let selection = checks::read_selection(selection)?;
    Ok(checks::feature_states(&manifests, &selection, path_var))
}

/// All the lines for `facts`, in the order they print.
pub fn lines(facts: &Facts) -> Vec<Line> {
    let mut lines = vec![
        system(&facts.host),
        compositor(facts.compositor),
        quickshell(&facts.quickshell, &Range::supported()),
        daemon(&facts.daemon),
        config(&facts.config),
    ];
    lines.extend(features_lines(&facts.features));
    lines.extend(render_loop(&facts.host, facts.render_loop.as_deref()));
    lines.extend(login_apps_line(&facts.daemon, &facts.startup));
    lines
}

/// `nova doctor`. Fails when any line does.
pub fn run(socket: &Path) -> Result<(), String> {
    let lines = lines(&Facts::gather(socket));
    for line in &lines {
        println!("{line}");
    }
    let count = |level| lines.iter().filter(|line| line.level == level).count();
    let (failed, warned) = (count(Level::Fail), count(Level::Warn));
    println!();
    match (failed, warned) {
        (0, 0) => println!("Everything looks right."),
        (0, _) => println!("No problems, {}.", plural(warned, "warning")),
        _ => return Err(format!("{}, {}.", plural(failed, "problem"), plural(warned, "warning"))),
    }
    Ok(())
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn system(host: &Host) -> Line {
    let gpus = match host.gpus.as_slice() {
        [] => "no GPU found".to_owned(),
        gpus => gpus.iter().map(|gpu| gpu_name(*gpu, host.nvidia_driver)).collect::<Vec<_>>().join(" + "),
    };
    let detail = format!("{}, {gpus}", host.distro);
    match host.distro {
        Distro::Other(_) => {
            Line::new(Level::Warn, "system", format!("{detail}: `{SETUP_COMMAND}` can't install packages here"))
        }
        _ => Line::new(Level::Ok, "system", detail),
    }
}

fn gpu_name(gpu: Gpu, nvidia_driver: bool) -> &'static str {
    match gpu {
        Gpu::Nvidia if nvidia_driver => "NVIDIA (nvidia driver)",
        Gpu::Nvidia => "NVIDIA (nouveau)",
        Gpu::Amd => "AMD",
        Gpu::Intel => "Intel",
    }
}

fn compositor(name: Option<&str>) -> Line {
    match name {
        Some(name) => Line::new(Level::Ok, "compositor", name),
        None => Line::new(
            Level::Warn,
            "compositor",
            "neither niri nor Hyprland found (NIRI_SOCKET, HYPRLAND_INSTANCE_SIGNATURE), so no workspaces will show",
        ),
    }
}

fn quickshell(version: &Result<String, String>, range: &Range) -> Line {
    match version {
        Err(problem) => Line::new(Level::Fail, "quickshell", format!("{problem}; install it with `{SETUP_COMMAND}`")),
        Ok(version) => match range.problem(version) {
            Some(problem) => Line::new(Level::Fail, "quickshell", problem),
            None => Line::new(Level::Ok, "quickshell", format!("{version} (tested: {range})")),
        },
    }
}

fn daemon(daemon: &Daemon) -> Line {
    let own = env!("CARGO_PKG_VERSION");
    match daemon {
        Daemon::Running { version, .. } if version == own => {
            Line::new(Level::Ok, "novad", format!("running {version}"))
        }
        Daemon::Running { version, .. } => Line::new(
            Level::Warn,
            "novad",
            format!("running {version}, but this nova is {own}; `{RESTART_COMMAND}` switches"),
        ),
        Daemon::NotRunning => Line::new(Level::Warn, "novad", "not running; start it with `nova`"),
        Daemon::NoAnswer(problem) => {
            Line::new(Level::Fail, "novad", format!("{problem}; `{RESTART_COMMAND}` starts it again"))
        }
    }
}

/// A file that doesn't parse fails: novad ignores all of it. A theme that
/// won't load only warns: novad shows its default theme instead.
fn config(read: &Result<Vec<String>, String>) -> Line {
    match read {
        Err(problem) => Line::new(Level::Fail, "config", format!("{problem}; novad ignores it until it's fixed")),
        Ok(problems) if problems.is_empty() => Line::new(Level::Ok, "config", "config.toml reads fine"),
        Ok(problems) => Line::new(Level::Warn, "config", problems.join("; ")),
    }
}

/// One line for all features when they're fine, one per feature that isn't.
fn features_lines(features: &Result<Vec<FeatureState>, String>) -> Vec<Line> {
    let features = match features {
        Ok(features) => features,
        Err(problem) => return vec![Line::new(Level::Fail, "features", problem.clone())],
    };
    let missing: Vec<Line> = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::MissingDeps)
        .map(|feature| {
            let needs: Vec<String> = feature.missing.iter().map(|entry| entry.replace('|', " or ")).collect();
            Line::new(
                Level::Fail,
                "features",
                format!("{} needs {}; `{SETUP_COMMAND}` installs it", feature.id, needs.join(", ")),
            )
        })
        .collect();
    if !missing.is_empty() {
        return missing;
    }
    let on: Vec<&str> = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::Ready)
        .map(|feature| feature.id.as_str())
        .collect();
    vec![Line::new(Level::Ok, "features", format!("{} on, nothing missing", on.join(", ")))]
}

/// Only on NVIDIA's driver, where Qt's threaded render loop costs the UI
/// memory.
fn render_loop(host: &Host, value: Option<&str>) -> Option<Line> {
    if !host.nvidia_driver || value == Some(RENDER_LOOP_FOR_NVIDIA) {
        return None;
    }
    Some(Line::new(
        Level::Warn,
        "nvidia",
        format!(
            "{RENDER_LOOP_VAR} isn't {RENDER_LOOP_FOR_NVIDIA} in this shell; on NVIDIA's driver the UI uses less memory with it"
        ),
    ))
}

/// Tray apps the compositor starts at login, which may come up before the
/// tray and miss it.
fn login_apps_line(daemon: &Daemon, startup: &[StartupCommand]) -> Option<Line> {
    let Daemon::Running { tray, .. } = daemon else { return None };
    let found = login_apps::tray_apps(startup, tray);
    if found.is_empty() {
        return None;
    }
    let apps: Vec<String> = found.iter().map(|(name, source)| format!("{name} ({source})")).collect();
    Some(Line::new(
        Level::Warn,
        "login apps",
        format!(
            "{} may start before Nova's tray and miss it; start them from {AUTOSTART_DIR} instead (the app's own \
             \"open at login\" setting writes there)",
            apps.join(", ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::tests::FakeRoot;

    fn host(distro: Distro, gpus: Vec<Gpu>, nvidia_driver: bool) -> Host {
        Host { distro, gpus, nvidia_driver }
    }

    fn feature(id: &str, status: FeatureStatus, missing: &[&str]) -> FeatureState {
        FeatureState { id: id.into(), status, missing: missing.iter().map(|m| m.to_string()).collect() }
    }

    fn healthy() -> Facts {
        Facts {
            host: host(Distro::Arch, vec![Gpu::Amd], false),
            compositor: Some("niri"),
            quickshell: Ok("0.3.2".into()),
            daemon: Daemon::Running { version: env!("CARGO_PKG_VERSION").into(), tray: vec![] },
            config: Ok(vec![]),
            features: Ok(vec![
                feature("ai-usage", FeatureStatus::Ready, &[]),
                feature("core", FeatureStatus::Ready, &[]),
                feature("recording", FeatureStatus::Off, &[]),
            ]),
            render_loop: None,
            startup: vec![],
        }
    }

    #[test]
    fn tray_apps_the_compositor_starts_warn() {
        let mut facts = healthy();
        facts.startup = ["Discord", "nova"]
            .map(|program| StartupCommand { source: "niri's spawn-at-startup", program: program.into() })
            .into();
        assert!(!rendered(&facts).contains("login apps"), "no tray: no warning");
        facts.daemon = Daemon::Running {
            version: env!("CARGO_PKG_VERSION").into(),
            tray: vec![TrayItem {
                address: ":1.5/StatusNotifierItem".into(),
                id: "discord_status_icon_1".into(),
                title: String::new(),
                app: None,
            }],
        };
        let text = rendered(&facts);
        assert!(
            text.contains(
                "warn  login apps  discord_status_icon_1 (niri's spawn-at-startup) may start before Nova's tray"
            ),
            "{text}"
        );
    }

    fn rendered(facts: &Facts) -> String {
        lines(facts).iter().map(|line| format!("{line}\n")).collect()
    }

    #[test]
    fn a_healthy_machine_is_all_ok() {
        let (range, version) = (Range::supported(), env!("CARGO_PKG_VERSION"));
        assert_eq!(
            rendered(&healthy()),
            format!(
                "ok    system      Arch, AMD\n\
                 ok    compositor  niri\n\
                 ok    quickshell  0.3.2 (tested: {range})\n\
                 ok    novad       running {version}\n\
                 ok    config      config.toml reads fine\n\
                 ok    features    ai-usage, core on, nothing missing\n"
            )
        );
    }

    #[test]
    fn quickshell_outside_the_range_or_missing_fails() {
        let range = Range::supported();
        assert_eq!(quickshell(&Ok("0.2.0".into()), &range).level, Level::Fail);
        assert_eq!(quickshell(&Ok("0.4.0".into()), &range).level, Level::Fail);
        assert_eq!(quickshell(&Ok("0.3.9".into()), &range).level, Level::Ok);
        let missing = quickshell(&Err("couldn't run quickshell --version: not found".into()), &range);
        assert_eq!(missing.level, Level::Fail);
        assert!(missing.detail.contains("nova setup"), "{missing}");
    }

    #[test]
    fn novad_down_warns_and_stuck_fails() {
        assert_eq!(daemon(&Daemon::NotRunning).level, Level::Warn);
        assert_eq!(daemon(&Daemon::NoAnswer("novad didn't say hello".into())).level, Level::Fail);
        let old = daemon(&Daemon::Running { version: "0.0.1".into(), tray: vec![] });
        assert_eq!(old.level, Level::Warn);
        assert!(old.detail.contains("nova restart"), "{old}");
    }

    #[test]
    fn reaching_novad_tells_gone_from_stuck() {
        let root = FakeRoot::new("doctor-reach", "", &[], false);
        let socket = root.0.join("novad.sock");
        assert!(matches!(reach(&socket), Daemon::NotRunning));
        // A socket file left by a novad that died.
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        assert!(matches!(reach(&socket), Daemon::NotRunning));
        // Listening but never answering: times out on hello.
        std::fs::remove_file(&socket).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(matches!(reach(&socket), Daemon::NoAnswer(_)));
    }

    #[test]
    fn missing_binaries_fail_per_feature() {
        let facts = Facts {
            features: Ok(vec![
                feature("core", FeatureStatus::Ready, &[]),
                feature("recording", FeatureStatus::MissingDeps, &["gpu-screen-recorder|wf-recorder"]),
            ]),
            ..healthy()
        };
        let text = rendered(&facts);
        assert!(
            text.contains(
                "FAIL  features    recording needs gpu-screen-recorder or wf-recorder; `nova setup` installs it"
            ),
            "{text}"
        );
        assert!(!text.contains("nothing missing"), "{text}");
    }

    #[test]
    fn broken_settings_fail() {
        let facts = Facts {
            config: Err("config.toml: unknown field `them`".into()),
            features: Err("features.toml: expected `=`".into()),
            ..healthy()
        };
        let failed: Vec<&str> =
            lines(&facts).into_iter().filter(|line| line.level == Level::Fail).map(|line| line.check).collect();
        assert_eq!(failed, ["config", "features"]);
    }

    #[test]
    fn the_render_loop_matters_only_on_nvidias_driver() {
        let nvidia = host(Distro::Arch, vec![Gpu::Nvidia], true);
        assert_eq!(render_loop(&nvidia, None).unwrap().level, Level::Warn);
        assert_eq!(render_loop(&nvidia, Some("threaded")).unwrap().level, Level::Warn);
        assert_eq!(render_loop(&nvidia, Some("basic")), None);
        assert_eq!(render_loop(&host(Distro::Arch, vec![Gpu::Nvidia], false), None), None);
        assert_eq!(render_loop(&host(Distro::Arch, vec![Gpu::Amd], false), None), None);
    }

    #[test]
    fn no_compositor_and_unknown_distros_warn() {
        let facts = Facts { compositor: None, host: host(Distro::Other("gentoo".into()), vec![], false), ..healthy() };
        let text = rendered(&facts);
        assert!(
            text.contains("warn  system      gentoo, no GPU found: `nova setup` can't install packages here"),
            "{text}"
        );
        assert!(text.contains("warn  compositor  neither niri nor Hyprland found"), "{text}");
    }

    fn config_line(path: &Path, themes: &Path) -> Line {
        super::config(&checks::read_config(path, themes))
    }

    /// The readers novad uses, on real files: a theme that doesn't exist and
    /// a feature whose binary isn't on PATH.
    #[test]
    fn settings_are_read_the_way_novad_reads_them() {
        let root = FakeRoot::new("doctor-settings", "ID=arch\n", &[], false);
        let config = root.0.join("config.toml");
        std::fs::write(&config, "[theme]\nname = \"nope\"\n").unwrap();
        let themes = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let problems = checks::read_config(&config, &themes).unwrap();
        assert!(problems.iter().any(|problem| problem.contains("nope")), "{problems:?}");
        assert_eq!(config_line(&config, &themes).level, Level::Warn);
        assert_eq!(checks::read_config(&root.0.join("missing.toml"), &themes), Ok(vec![]));
        std::fs::write(&config, "[them]\n").unwrap();
        assert_eq!(config_line(&config, &themes).level, Level::Fail);

        let selection = root.0.join("features.toml");
        std::fs::write(&selection, "enabled = [\"recording\"]\n").unwrap();
        let manifests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let states = features(&manifests, &selection, "/nonexistent").unwrap();
        let recording = states.iter().find(|state| state.id == "recording").unwrap();
        assert_eq!(recording.status, FeatureStatus::MissingDeps);

        std::fs::write(&selection, "enabled = [\n").unwrap();
        assert!(features(&manifests, &selection, "/nonexistent").is_err());
    }
}
