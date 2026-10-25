//! Which app a tray icon belongs to, as names to look up in the launcher's
//! apps (`Launcher::find_app`), from the process that owns the icon on the
//! bus. Apps' own tray ids can't be trusted for this: Electron names its
//! icon after the app's internal name (`Claude_status_icon_1` for
//! `com.anthropic.Claude.desktop`), so they only come last.
//!
//! In order, all lower case and without repeats:
//! 1. The process's names: its program file, `argv[0]` and `comm`, as a
//!    window id would carry them (`claude-desktop`, `steam`).
//! 2. Its systemd unit, which names the desktop entry it was started from
//!    (`app-discord-375306.scope`, `app-nova-steam-4534754.scope`,
//!    `app-blueman@autostart.service`). It comes second because an app
//!    started from a terminal sits in the terminal's unit.
//! 3. The icon's own id, without Electron's `_status_icon_<n>`. It comes
//!    before the unit when the process names were all shared runtimes
//!    (`python3`): such an app started from a terminal would otherwise be
//!    taken for the terminal.

use std::path::Path;

/// Program names that say nothing about the app: shared runtimes and
/// sandbox helpers that run many apps.
const GENERIC_PROGRAMS: &[&str] = &["electron", "xdg-dbus-proxy", "bwrap", "python", "java"];
/// What Electron appends to its tray icons' ids, before a number.
const ELECTRON_ID_SUFFIX: &str = "_status_icon_";

/// The names to look the app up by, for the process `pid` (read from
/// `proc`, normally `/proc`) whose tray icon has the id `icon_id`.
pub fn candidates(proc: &Path, pid: Option<u32>, icon_id: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(pid) = pid {
        let dir = proc.join(pid.to_string());
        let exe = std::fs::read_link(dir.join("exe")).ok();
        let argv0 = std::fs::read(dir.join("cmdline")).ok().and_then(|cmdline| {
            cmdline.split(|&byte| byte == 0).next().map(|arg| String::from_utf8_lossy(arg).into_owned())
        });
        let comm = std::fs::read_to_string(dir.join("comm")).ok();
        let programs = [exe.as_deref().and_then(Path::to_str), argv0.as_deref(), comm.as_deref()];
        names.extend(programs.into_iter().flatten().filter_map(program_name));
        if names.is_empty() {
            names.extend(icon_app_id(icon_id));
        }
        if let Ok(cgroup) = std::fs::read_to_string(dir.join("cgroup")) {
            names.extend(unit_app_ids(&cgroup));
        }
    }
    names.extend(icon_app_id(icon_id));
    let mut seen = Vec::new();
    for name in names.into_iter().map(|name| name.to_lowercase()) {
        if !name.is_empty() && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// The app a program path or name stands for, unless it's a shared
/// runtime (whatever its version: `python3.14`). Nix wraps programs as
/// `.<name>-wrapped` (cut to `.<name>-wrappe` in the kernel's 15-character
/// `comm`).
fn program_name(program: &str) -> Option<String> {
    let name = program.trim().rsplit('/').next()?;
    let name = name.strip_prefix('.').unwrap_or(name);
    let name = name.strip_suffix("-wrapped").or_else(|| name.strip_suffix("-wrappe")).unwrap_or(name);
    let unversioned = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.').to_lowercase();
    (!name.is_empty() && !GENERIC_PROGRAMS.contains(&unversioned.as_str())).then(|| name.to_string())
}

/// The app ids the process's systemd unit names, from its `cgroup` file.
/// Units follow systemd's scheme for desktop apps,
/// `app[-<launcher>]-<app id>[@<random>].service` or
/// `app[-<launcher>]-<app id>-<random>.scope`, with `-` in names escaped
/// as `\x2d`. Whether there's a launcher part can't be told, so both
/// readings come out, the whole one first.
fn unit_app_ids(cgroup: &str) -> Vec<String> {
    let Some(unit) = cgroup
        .lines()
        .filter_map(|line| line.rsplit('/').next())
        .find(|unit| unit.starts_with("app-") && (unit.ends_with(".scope") || unit.ends_with(".service")))
    else {
        return vec![];
    };
    let name = unit.strip_prefix("app-").unwrap_or(unit);
    let name = match name.strip_suffix(".scope") {
        Some(scope) => scope.rsplit_once('-').map_or(scope, |(name, _random)| name),
        None => {
            let service = name.strip_suffix(".service").unwrap_or(name);
            service.split_once('@').map_or(service, |(name, _instance)| name)
        }
    };
    let mut ids = vec![name.replace("\\x2d", "-")];
    if let Some((_launcher, id)) = name.split_once('-') {
        ids.push(id.replace("\\x2d", "-"));
    }
    ids
}

/// The app an icon's own id names: Electron's `<app>_status_icon_<n>`
/// gives `<app>`, anything else is taken as it is.
fn icon_app_id(icon_id: &str) -> Option<String> {
    let id = match icon_id.rsplit_once(ELECTRON_ID_SUFFIX) {
        Some((app, number)) if number.chars().all(|c| c.is_ascii_digit()) => app,
        _ => icon_id,
    };
    (!id.is_empty()).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Units and names seen on a NixOS + niri desktop (2026-10-25).

    #[test]
    fn units_name_the_app_with_and_without_a_launcher() {
        let ids =
            |unit: &str| unit_app_ids(&format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}\n"));
        assert_eq!(ids("app-discord-375306.scope"), ["discord"]);
        assert_eq!(ids("app-com.anthropic.Claude-829376.scope"), ["com.anthropic.Claude"]);
        assert_eq!(ids("app-nova-steam-4534754.scope"), ["nova-steam", "steam"]);
        assert_eq!(ids("app-niri-Discord-375276.scope"), ["niri-Discord", "Discord"]);
        assert_eq!(ids("app-blueman@autostart.service"), ["blueman"]);
        assert_eq!(
            ids("app-gnome-org.gnome.Calculator@1a2b.service"),
            ["gnome-org.gnome.Calculator", "org.gnome.Calculator"]
        );
        assert_eq!(ids("app-my\\x2dapp-123.scope"), ["my-app"]);
        assert!(ids("nova.service").is_empty());
        assert!(unit_app_ids("").is_empty());
    }

    #[test]
    fn nix_wrappers_and_runtimes() {
        assert_eq!(program_name("/nix/store/c42f-discord/opt/Discord/.Discord-wrapped").as_deref(), Some("Discord"));
        assert_eq!(program_name(".ferdium-wrappe\n").as_deref(), Some("ferdium"));
        assert_eq!(program_name("claude-desktop").as_deref(), Some("claude-desktop"));
        assert_eq!(program_name("/usr/lib/electron33/electron"), None);
        assert_eq!(program_name("Electron"), None);
        assert_eq!(program_name("/nix/store/xahx-python3-3.14.8/bin/python3.14"), None);
    }

    #[test]
    fn electron_icon_ids_lose_their_suffix() {
        assert_eq!(icon_app_id("discord_status_icon_1").as_deref(), Some("discord"));
        assert_eq!(icon_app_id("Claude_status_icon_12").as_deref(), Some("Claude"));
        assert_eq!(icon_app_id("ferdium").as_deref(), Some("ferdium"));
        assert_eq!(icon_app_id(""), None);
    }

    #[test]
    fn a_process_gives_its_names_then_its_unit_then_the_icon_id() {
        let proc = std::env::temp_dir().join(format!("nova-tray-proc-{}", std::process::id()));
        let dir = proc.join("375306");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cmdline"), b"/nix/store/c42f/opt/Discord/.Discord-wrapped\0--type=browser\0").unwrap();
        std::fs::write(dir.join("comm"), ".Discord-wrappe\n").unwrap();
        std::fs::write(dir.join("cgroup"), "0::/user.slice/app.slice/app-discord-375306.scope\n").unwrap();
        assert_eq!(candidates(&proc, Some(375306), "discord_status_icon_1"), ["discord"]);
        assert_eq!(candidates(&proc, Some(1), "Claude_status_icon_1"), ["claude"]);
        assert_eq!(candidates(&proc, None, ""), Vec::<String>::new());
        // A runtime's app started from a terminal sits in the terminal's unit.
        let script = proc.join("4242");
        std::fs::create_dir_all(&script).unwrap();
        std::fs::write(script.join("cmdline"), b"python3 /home/me/bin/my-tray-app ").unwrap();
        std::fs::write(script.join("comm"), "python3\n").unwrap();
        std::fs::write(script.join("cgroup"), "0::/user.slice/app.slice/app-kitty-123.scope\n").unwrap();
        assert_eq!(candidates(&proc, Some(4242), "my-tray-app"), ["my-tray-app", "kitty"]);
        std::fs::remove_dir_all(&proc).unwrap();
    }
}
