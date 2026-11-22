//! `nova doctor`: one line per check, ok, warn or fail, and a non-zero exit
//! when anything fails. A warning is something Nova runs with but works
//! worse for, like no compositor it follows.
//!
//! Facts are gathered first (`Facts::gather`), then each check turns its
//! facts into a line, so the checks are tested on made-up facts.

use crate::client::{Client, Unreachable};
use crate::host::{Distro, Host};
use crate::lifecycle::RESTART_COMMAND;
use crate::login_apps::{self, AUTOSTART_DIR, StartupCommand};
use nova_daemon_info::{FeatureState, FeatureStatus};
use nova_features::Gpu;
use nova_quickshell::Range;
use nova_tray_api::TrayItem;
use novad::checks;
use std::fmt;
use std::path::{Path, PathBuf};

/// Qt's render loop variable, and the value that keeps the UI's memory down
/// on NVIDIA's driver.
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
            features: features(&nova_features::default_dir(), &nova_paths::config_files("features.toml"), &path_var),
            startup: config_dir.parent().map(login_apps::read).unwrap_or_default(),
        }
    }
}

/// Tells "not running" from "stuck" by how the one connection fails
/// ([`Client::reach`]), so the doctor never makes a second connection that
/// could wait on a stuck novad.
fn reach(socket: &Path) -> Daemon {
    match Client::reach(socket) {
        Err(Unreachable::NotRunning) => Daemon::NotRunning,
        Err(Unreachable::NoAnswer(problem)) => Daemon::NoAnswer(problem),
        Ok(mut client) => {
            // Only for a warning: a novad too old to answer is still running.
            let events = client.current(vec![nova_tray_api::TOPIC]).unwrap_or_default();
            let tray = events
                .into_iter()
                .find_map(|event| match event {
                    crate::client::Sent::Tray(nova_tray_api::Event::Tray(tray)) => Some(tray.items),
                    _ => None,
                })
                .unwrap_or_default();
            Daemon::Running { version: client.daemon_version, tray }
        }
    }
}

fn features(manifests_dir: &Path, selection: &[PathBuf], path_var: &str) -> Result<Vec<FeatureState>, String> {
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
    lines.extend(features_lines(&facts.features, &facts.host.distro));
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
fn features_lines(features: &Result<Vec<FeatureState>, String>, distro: &Distro) -> Vec<Line> {
    let features = match features {
        Ok(features) => features,
        Err(problem) => return vec![Line::new(Level::Fail, "features", problem.clone())],
    };
    let missing: Vec<Line> = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::MissingDeps)
        .map(|feature| {
            let needs: Vec<String> = feature.missing.iter().map(|entry| entry.replace('|', " or ")).collect();
            let install = match distro {
                Distro::NixOs => format!(
                    "Nova's Nix module installs it with `programs.nova.features.{}.enable = true;` and a rebuild",
                    feature.id
                ),
                _ => format!("`{SETUP_COMMAND}` installs it"),
            };
            Line::new(Level::Fail, "features", format!("{} needs {}; {install}", feature.id, needs.join(", ")))
        })
        .collect();
    let unknown = features.iter().filter(|feature| feature.status != FeatureStatus::Off).flat_map(|feature| {
        feature.slots.iter().filter(|slot| nova_features::host(&slot.host).is_none()).map(|slot| {
            Line::new(
                Level::Warn,
                "features",
                format!(
                    "{} puts {} in {}, a slot host Nova doesn't have, so it never shows",
                    feature.id, slot.view, slot.host
                ),
            )
        })
    });
    // Features whose hosts are all unknown are warned about above.
    let unhosted =
        features.iter().filter(|feature| nova_features::has_no_host(feature, features)).filter_map(|feature| {
            let mut owners: Vec<&FeatureState> = feature
                .slots
                .iter()
                .filter_map(|slot| nova_features::host(&slot.host))
                .filter_map(|host| host.owner)
                .filter_map(|owner| features.iter().find(|other| other.id == owner))
                .collect();
            owners.sort_unstable_by(|a, b| a.id.cmp(&b.id));
            owners.dedup_by(|a, b| a.id == b.id);
            if owners.is_empty() {
                return None;
            }
            let ids: Vec<&str> = owners.iter().map(|owner| owner.id.as_str()).collect();
            let why = match owners.iter().all(|owner| owner.status == FeatureStatus::Off) {
                true => format!("{} is off", ids.join(" and ")),
                false => format!("{} isn't ready", ids.join(" and ")),
            };
            Some(Line::new(Level::Warn, "features", format!("{} is on but has nowhere to show: {why}", feature.id)))
        });
    let problems: Vec<Line> = missing.into_iter().chain(unknown).chain(unhosted).collect();
    if !problems.is_empty() {
        return problems;
    }
    let on: Vec<&str> = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::Ready)
        .map(|feature| feature.id.as_str())
        .collect();
    if on.is_empty() {
        return vec![Line::new(Level::Ok, "features", "all off, so novad runs without a UI")];
    }
    vec![Line::new(Level::Ok, "features", format!("{} on, nothing missing", on.join(", ")))]
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
        FeatureState {
            id: id.into(),
            status,
            missing: missing.iter().map(|m| m.to_string()).collect(),
            slots: vec![],
            mode: None,
        }
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
                feature("bar", FeatureStatus::Ready, &[]),
                feature("recording", FeatureStatus::Off, &[]),
            ]),
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
                menu_only: false,
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
                 ok    features    ai-usage, bar on, nothing missing\n"
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
        // A child another test forks meanwhile holds the listener until it
        // runs its program.
        let refused = || std::os::unix::net::UnixStream::connect(&socket).is_err();
        nova_test_support::wait_until(std::time::Duration::from_secs(5), "the listener to close", refused);
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
                feature("bar", FeatureStatus::Ready, &[]),
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

        let nixos = Facts { host: host(Distro::NixOs, vec![Gpu::Nvidia], true), ..facts };
        let text = rendered(&nixos);
        assert!(text.contains("recording needs gpu-screen-recorder or wf-recorder; Nova's Nix module installs it with `programs.nova.features.recording.enable = true;` and a rebuild"), "{text}");
    }

    /// A slot naming a host Nova doesn't have, and a feature whose hosts'
    /// feature is off (now playing with the bar off), each warn; with
    /// everything off the doctor says the UI doesn't run.
    #[test]
    fn slots_with_nowhere_to_show_warn() {
        let slot = |host: &str, view: &str| nova_features::Slot { host: host.into(), view: view.into(), order: 0 };
        let media =
            FeatureState { slots: vec![slot("bar.left", "media.pill")], ..feature("media", FeatureStatus::Ready, &[]) };
        let typo = FeatureState {
            slots: vec![slot("bar.middle", "clock.big"), slot("shell.overlays", "clock.overlay")],
            ..feature("clock", FeatureStatus::Ready, &[])
        };
        let facts = Facts { features: Ok(vec![feature("bar", FeatureStatus::Off, &[]), media, typo]), ..healthy() };
        let text = rendered(&facts);
        assert!(text.contains("warn  features    media is on but has nowhere to show: bar is off"), "{text}");
        assert!(
            text.contains("clock puts clock.big in bar.middle, a slot host Nova doesn't have, so it never shows"),
            "{text}"
        );

        // Only an unknown host: one line, not a second "nowhere to show".
        let stray = FeatureState {
            slots: vec![slot("bar.middle", "stray.pill")],
            ..feature("stray", FeatureStatus::Ready, &[])
        };
        let text = rendered(&Facts { features: Ok(vec![stray]), ..healthy() });
        assert!(!text.contains("nowhere to show"), "{text}");
        // A host's feature that's on but missing something isn't "off".
        let sound = FeatureState {
            slots: vec![slot("quick-settings.sliders", "sound.slider")],
            ..feature("sound", FeatureStatus::Ready, &[])
        };
        let qs = feature("quick-settings", FeatureStatus::MissingDeps, &[]);
        let text = rendered(&Facts { features: Ok(vec![qs, sound]), ..healthy() });
        assert!(text.contains("sound is on but has nowhere to show: quick-settings isn't ready"), "{text}");

        let all_off = Facts { features: Ok(vec![feature("bar", FeatureStatus::Off, &[])]), ..healthy() };
        assert!(rendered(&all_off).contains("ok    features    all off, so novad runs without a UI"));
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
        let states = features(&manifests, std::slice::from_ref(&selection), "/nonexistent").unwrap();
        let recording = states.iter().find(|state| state.id == "recording").unwrap();
        assert_eq!(recording.status, FeatureStatus::MissingDeps);

        std::fs::write(&selection, "enabled = [\n").unwrap();
        assert!(features(&manifests, std::slice::from_ref(&selection), "/nonexistent").is_err());
    }
}
