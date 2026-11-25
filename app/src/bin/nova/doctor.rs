use crate::host::{Distro, Host};
use crate::login_apps::{self, AUTOSTART_DIR, StartupCommand};
use nova_cli::client::{Client, RESTART_COMMAND, Unreachable};
use nova_daemon_info::{FeatureState, FeatureStatus, UiState, UiStatus};
use nova_desktop_info::HYPRLAND_LOCK_RESTORE;
use nova_features::Gpu;
use nova_tray_api::TrayItem;
use novad::checks;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const SETUP_COMMAND: &str = "nova setup";
const NIXOS_HYPRLAND_RULES: &str = "/etc/xdg/nova/hyprland.lua";
const FC_MATCH_TIMEOUT: Duration = Duration::from_secs(10);

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
        write!(f, "{level:<6}{:<12}{}", self.check, nova_cli::status::printable(&self.detail))
    }
}

#[derive(Debug)]
pub enum Daemon {
    Running { version: String, tray: Vec<TrayItem>, ui: Option<UiStatus> },
    NotRunning,
    NoAnswer(String),
}

pub struct Facts {
    pub host: Host,
    pub compositor: Option<&'static str>,
    pub ui: Option<PathBuf>,
    pub daemon: Daemon,
    pub config: Result<Vec<String>, String>,
    pub features: Result<Vec<FeatureState>, String>,
    pub startup: Vec<StartupCommand>,
    pub lock_restore: Option<Result<bool, String>>,
    pub fonts: Result<Vec<(&'static str, String)>, String>,
}

impl Facts {
    pub fn gather(socket: &Path) -> Self {
        let path_var = checks::session_path();
        let config_dir = nova_paths::config_dir();
        Self {
            host: Host::detect(Path::new("/")),
            compositor: checks::compositor(|name| std::env::var_os(name)),
            ui: nova_paths::find_program(checks::GPUI_UI, &path_var),
            daemon: reach(socket),
            config: checks::read_config(&config_dir.join("config.toml"), &checks::default_themes_dir()),
            features: features(&nova_features::default_dir(), &nova_paths::config_files("features.toml"), &path_var),
            startup: config_dir.parent().map(login_apps::read).unwrap_or_default(),
            lock_restore: checks::restores_session_lock(|name| std::env::var_os(name)),
            fonts: match_fonts(&path_var),
        }
    }
}

fn match_fonts(path_var: &str) -> Result<Vec<(&'static str, String)>, String> {
    nova_theme_info::FONTS
        .into_iter()
        .map(|family| {
            let mut command = Command::new("fc-match");
            command.args(["--format", "%{family}", family]).env("PATH", path_var);
            let output =
                checks::output_within(command, FC_MATCH_TIMEOUT).map_err(|e| format!("can't run fc-match: {e}"))?;
            Ok((family, String::from_utf8_lossy(&output.stdout).into_owned()))
        })
        .collect()
}

fn reach(socket: &Path) -> Daemon {
    match Client::reach(socket) {
        Err(Unreachable::NotRunning) => Daemon::NotRunning,
        Err(Unreachable::NoAnswer(problem)) => Daemon::NoAnswer(problem),
        Ok(mut client) => {
            let ui = client.status().ok().map(|status| status.ui);
            let events = client.current(vec![nova_tray_api::TOPIC]).unwrap_or_default();
            let tray = events
                .into_iter()
                .find_map(|event| match event {
                    nova_cli::client::Sent::Tray(nova_tray_api::Event::Tray(tray)) => Some(tray.items),
                    _ => None,
                })
                .unwrap_or_default();
            Daemon::Running { version: client.daemon_version, tray, ui }
        }
    }
}

fn features(manifests_dir: &Path, selection: &[PathBuf], path_var: &str) -> Result<Vec<FeatureState>, String> {
    let manifests = nova_features::load_all(manifests_dir).map_err(|e| e.to_string())?;
    let selection = checks::read_selection(selection)?;
    Ok(checks::feature_states(&manifests, &selection, path_var))
}

pub fn lines(facts: &Facts) -> Vec<Line> {
    let mut lines = vec![
        system(&facts.host),
        compositor(facts.compositor),
        ui(facts.ui.as_deref(), &facts.daemon),
        daemon(&facts.daemon),
        config(&facts.config),
        fonts(&facts.fonts, &facts.host.distro),
    ];
    lines.extend(features_lines(&facts.features, &facts.host.distro));
    lines.extend(login_apps_line(&facts.daemon, &facts.startup));
    lines.extend(lock_restore_line(facts));
    lines
}

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

fn ui(program: Option<&Path>, daemon: &Daemon) -> Line {
    let since = |status: &UiStatus| status.since.as_deref().map(|since| format!(" since {since}")).unwrap_or_default();
    match daemon {
        Daemon::Running { ui: Some(status), .. } => match status.state {
            UiState::Running => {
                let pid = status.pid.map(|pid| format!(", pid {pid}")).unwrap_or_default();
                Line::new(Level::Ok, "ui", format!("running{}{pid}", since(status)))
            }
            UiState::Restarting => {
                let problem = status.problem.as_deref().unwrap_or("novad is starting it again");
                Line::new(Level::Fail, "ui", format!("not running{}: {problem}", since(status)))
            }
            UiState::Disabled => Line::new(Level::Warn, "ui", "novad runs without its UI (--no-ui)"),
            UiState::NotNeeded => Line::new(Level::Ok, "ui", "not running: no feature with a UI is on"),
        },
        _ => match program {
            Some(program) => Line::new(Level::Ok, "ui", program.display().to_string()),
            None => Line::new(Level::Fail, "ui", format!("{} isn't on PATH, so Nova has no UI", checks::GPUI_UI)),
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

fn config(read: &Result<Vec<String>, String>) -> Line {
    match read {
        Err(problem) => Line::new(Level::Fail, "config", format!("{problem}; novad ignores it until it's fixed")),
        Ok(problems) if problems.is_empty() => Line::new(Level::Ok, "config", "config.toml reads fine"),
        Ok(problems) => Line::new(Level::Warn, "config", problems.join("; ")),
    }
}

fn fonts(matched: &Result<Vec<(&'static str, String)>, String>, distro: &Distro) -> Line {
    let matched = match matched {
        Ok(matched) => matched,
        Err(problem) => {
            return Line::new(Level::Warn, "fonts", format!("{problem}, so the look's fonts weren't checked"));
        }
    };
    let missing: Vec<String> = matched
        .iter()
        .filter(|(family, drawn)| !drawn.split(',').any(|name| name.trim().eq_ignore_ascii_case(family)))
        .map(|(family, drawn)| match drawn.split(',').next().map(str::trim).filter(|name| !name.is_empty()) {
            Some(drawn) => format!("{family} (draws {drawn})"),
            None => (*family).to_owned(),
        })
        .collect();
    let families: Vec<&str> = matched.iter().map(|(family, _)| *family).collect();
    if missing.is_empty() {
        return Line::new(Level::Ok, "fonts", families.join(", "));
    }
    let install = match distro {
        Distro::NixOs => "Nova's Nix module installs them with `programs.nova.fonts.enable = true;` and a rebuild",
        _ => "install them from your distro's packages or fonts.google.com",
    };
    Line::new(Level::Warn, "fonts", format!("{} not installed; {install}", missing.join(", ")))
}

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

fn lock_restore_line(facts: &Facts) -> Option<Line> {
    let on = facts.features.as_ref().is_ok_and(|features| {
        features.iter().any(|feature| feature.id == nova_lock::FEATURE_ID && feature.status != FeatureStatus::Off)
    });
    let compositor = facts.compositor.filter(|_| on)?;
    match facts.lock_restore.as_ref()? {
        Ok(true) => None,
        Err(problem) => Some(Line::new(
            Level::Warn,
            "lock",
            format!("can't tell whether a lock screen that crashed can lock again: {problem}"),
        )),
        Ok(false) => {
            let load = match facts.host.distro {
                Distro::NixOs => format!(
                    "Nova's home-manager module loads them, or with its NixOS module add `require(\"{NIXOS_HYPRLAND_RULES}\")` to \
                     hyprland.lua"
                ),
                _ => format!("`{SETUP_COMMAND}` loads them"),
            };
            Some(Line::new(
                Level::Fail,
                "lock",
                format!(
                    "{compositor}'s {HYPRLAND_LOCK_RESTORE} is off, so a lock screen that crashes leaves you locked out until a \
                     TTY; Nova's Hyprland rules turn it on, and {load}"
                ),
            ))
        }
    }
}

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
            ui: Some(PathBuf::from("/usr/bin/nova-ui")),
            daemon: Daemon::Running { version: env!("CARGO_PKG_VERSION").into(), tray: vec![], ui: None },
            config: Ok(vec![]),
            features: Ok(vec![
                feature("ai-usage", FeatureStatus::Ready, &[]),
                feature("bar", FeatureStatus::Ready, &[]),
                feature("recording", FeatureStatus::Off, &[]),
            ]),
            startup: vec![],
            lock_restore: Some(Ok(true)),
            fonts: Ok(nova_theme_info::FONTS.into_iter().map(|family| (family, family.to_owned())).collect()),
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
                icon: None,
                attention: false,
                has_menu: false,
                is_menu: false,
            }],
            ui: None,
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
    fn hyprland_that_wont_let_the_lock_screen_back_fails_while_the_lock_is_on() {
        let mut facts = healthy();
        (facts.host.distro, facts.compositor, facts.lock_restore) = (Distro::NixOs, Some("Hyprland"), Some(Ok(false)));
        assert!(!rendered(&facts).contains("lock        "), "the lock is off: no line");
        facts.features = Ok(vec![feature("lock", FeatureStatus::Ready, &[])]);
        let text = rendered(&facts);
        assert!(
            text.contains(
                "FAIL  lock        Hyprland's misc:allow_session_lock_restore is off, so a lock screen that crashes \
                 leaves you locked out until a TTY"
            ),
            "{text}"
        );
        assert!(text.contains(r#"add `require("/etc/xdg/nova/hyprland.lua")` to hyprland.lua"#), "{text}");
        facts.host.distro = Distro::Arch;
        assert!(rendered(&facts).contains("`nova setup` loads them"));
        facts.lock_restore = Some(Err("can't reach Hyprland".into()));
        assert!(
            rendered(&facts).contains("warn  lock        can't tell whether a lock screen that crashed can lock again")
        );
        facts.lock_restore = Some(Ok(true));
        assert!(!rendered(&facts).contains("lock        "), "{}", rendered(&facts));
    }

    #[test]
    fn a_healthy_machine_is_all_ok() {
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            rendered(&healthy()),
            format!(
                "ok    system      Arch, AMD\n\
                 ok    compositor  niri\n\
                 ok    ui          /usr/bin/nova-ui\n\
                 ok    novad       running {version}\n\
                 ok    config      config.toml reads fine\n\
                 ok    fonts       Inter, JetBrains Mono, Instrument Serif\n\
                 ok    features    ai-usage, bar on, nothing missing\n"
            )
        );
    }

    #[test]
    fn a_font_fontconfig_swaps_for_another_warns_with_how_to_install_it() {
        let mut facts = healthy();
        facts.fonts = Ok(vec![
            ("Inter", "Inter,Inter Display".into()),
            ("JetBrains Mono", "DejaVu Sans Mono".into()),
            ("Instrument Serif", "DejaVu Serif".into()),
        ]);
        assert!(
            rendered(&facts).contains(
                "warn  fonts       JetBrains Mono (draws DejaVu Sans Mono), Instrument Serif (draws DejaVu Serif) \
                 not installed; install them from your distro's packages or fonts.google.com"
            ),
            "{}",
            rendered(&facts)
        );
        facts.host.distro = Distro::NixOs;
        assert!(rendered(&facts).contains("`programs.nova.fonts.enable = true;`"), "{}", rendered(&facts));
        facts.fonts = Err("can't run fc-match: not found".into());
        assert!(
            rendered(&facts)
                .contains("warn  fonts       can't run fc-match: not found, so the look's fonts weren't checked")
        );
    }

    #[test]
    fn a_missing_nova_ui_fails() {
        let missing = rendered(&Facts { ui: None, ..healthy() });
        assert!(missing.contains("FAIL  ui          nova-ui isn't on PATH, so Nova has no UI"), "{missing}");
    }

    #[test]
    fn the_ui_line_asks_novad() {
        let with_ui = |state, problem: Option<&str>| {
            let ui = UiStatus {
                state,
                pid: (state == UiState::Running).then_some(7),
                pss_kib: None,
                restarts: 0,
                last_crash: None,
                since: Some("2026-11-23T21:00:00.000Z".into()),
                problem: problem.map(str::to_owned),
            };
            let daemon = Daemon::Running { version: env!("CARGO_PKG_VERSION").into(), tray: vec![], ui: Some(ui) };
            rendered(&Facts { daemon, ..healthy() })
        };
        let running = with_ui(UiState::Running, None);
        assert!(running.contains("ok    ui          running since 2026-11-23T21:00:00.000Z, pid 7"), "{running}");
        let missing = with_ui(UiState::Restarting, Some("it didn't start: couldn't start /nonexistent/nova-ui"));
        assert!(
            missing.contains(
                "FAIL  ui          not running since 2026-11-23T21:00:00.000Z: it didn't start: couldn't start /nonexistent/nova-ui"
            ),
            "{missing}"
        );
        let not_needed = with_ui(UiState::NotNeeded, None);
        assert!(not_needed.contains("ok    ui          not running: no feature with a UI is on"), "{not_needed}");
    }

    #[test]
    fn novad_down_warns_and_stuck_fails() {
        assert_eq!(daemon(&Daemon::NotRunning).level, Level::Warn);
        assert_eq!(daemon(&Daemon::NoAnswer("novad didn't say hello".into())).level, Level::Fail);
        let old = daemon(&Daemon::Running { version: "0.0.1".into(), tray: vec![], ui: None });
        assert_eq!(old.level, Level::Warn);
        assert!(old.detail.contains("nova restart"), "{old}");
    }

    #[test]
    fn reaching_novad_tells_gone_from_stuck() {
        let root = FakeRoot::new("doctor-reach", "", &[], false);
        let socket = root.0.join("novad.sock");
        assert!(matches!(reach(&socket), Daemon::NotRunning));
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        let refused = || std::os::unix::net::UnixStream::connect(&socket).is_err();
        nova_test_support::wait_until(std::time::Duration::from_secs(5), "the listener to close", refused);
        assert!(matches!(reach(&socket), Daemon::NotRunning));
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

        let stray = FeatureState {
            slots: vec![slot("bar.middle", "stray.pill")],
            ..feature("stray", FeatureStatus::Ready, &[])
        };
        let text = rendered(&Facts { features: Ok(vec![stray]), ..healthy() });
        assert!(!text.contains("nowhere to show"), "{text}");
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

    #[test]
    fn settings_are_read_the_way_novad_reads_them() {
        let root = FakeRoot::new("doctor-settings", "ID=arch\n", &[], false);
        let config = root.0.join("config.toml");
        std::fs::write(&config, "[theme]\nname = \"nope\"\n").unwrap();
        let themes = Path::new(env!("CARGO_MANIFEST_DIR")).join("../themes");
        let problems = checks::read_config(&config, &themes).unwrap();
        assert!(problems.iter().any(|problem| problem.contains("nope")), "{problems:?}");
        assert_eq!(config_line(&config, &themes).level, Level::Warn);
        assert_eq!(checks::read_config(&root.0.join("missing.toml"), &themes), Ok(vec![]));
        std::fs::write(&config, "[them]\n").unwrap();
        assert_eq!(config_line(&config, &themes).level, Level::Fail);

        let selection = root.0.join("features.toml");
        std::fs::write(&selection, "enabled = [\"recording\"]\n").unwrap();
        let manifests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
        let states = features(&manifests, std::slice::from_ref(&selection), "/nonexistent").unwrap();
        let recording = states.iter().find(|state| state.id == "recording").unwrap();
        assert_eq!(recording.status, FeatureStatus::MissingDeps);

        std::fs::write(&selection, "enabled = [\n").unwrap();
        assert!(features(&manifests, std::slice::from_ref(&selection), "/nonexistent").is_err());
    }
}
