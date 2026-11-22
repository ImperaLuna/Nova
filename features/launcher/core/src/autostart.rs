//! XDG autostart, as the freedesktop.org Desktop Application Autostart
//! Specification describes it: the apps the user wants at login, from
//! `autostart/` in each XDG config folder (the user's first, whose file
//! replaces the system's of the same name).
//!
//! With systemd, its generator starts them (`xdg-desktop-autostart.target`),
//! after novad is ready: nova.service is `Type=notify`, and Nova's drop-in
//! for `app-*.service` orders them after it. Without systemd, `nova` starts
//! novad with `--autostart`, and novad starts them itself once the tray's
//! name is claimed, once a session. Either way an app that looks for a tray
//! only at startup (Electron) finds one (IMP-107).

use nova_apps::desktop_entry::{self, Context, Entry, Locale, Skipped};
use nova_apps::launch::{self, Runner};
use nova_log::Logger;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const LOG_SOURCE: &str = "autostart";
const FOLDER: &str = "autostart";
/// The spec's default for an unset `XDG_CONFIG_DIRS`.
const DEFAULT_CONFIG_DIRS: &str = "/etc/xdg";
const ENTRY_EXTENSION: &str = "desktop";
/// systemd's target for the autostart units its generator makes.
const SYSTEMD_TARGET: &str = "xdg-desktop-autostart.target";
/// How long systemd gets to say whether that target runs.
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(2);

/// The autostart folders, in the order they're searched.
pub fn dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(nova_paths::config_home());
    let system = nova_paths::path_var("XDG_CONFIG_DIRS").unwrap_or_else(|| DEFAULT_CONFIG_DIRS.into());
    dirs.extend(std::env::split_paths(&system));
    let mut seen = BTreeSet::new();
    dirs.retain(|dir| dir.is_absolute() && seen.insert(dir.clone()));
    dirs.into_iter().map(|dir| dir.join(FOLDER)).collect()
}

/// The apps to start from `dirs`, in file name order, and the problems with
/// entries that aren't valid, for the log. The first folder with a file name
/// decides it, so a user's copy with `Hidden=true` turns a system one off.
pub fn entries(dirs: &[PathBuf], context: &Context) -> (Vec<Entry>, Vec<String>) {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut decided = BTreeSet::new();
    for dir in dirs {
        let Ok(read) = std::fs::read_dir(dir) else { continue };
        let mut paths: Vec<PathBuf> = read.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_name().and_then(|name| name.to_str()).map(str::to_string) else { continue };
            if path.extension().is_some_and(|extension| extension == ENTRY_EXTENSION)
                && path.is_file()
                && decided.insert(name.clone())
            {
                files.push((name, path));
            }
        }
    }
    files.sort();
    let mut apps = Vec::new();
    let mut problems = Vec::new();
    for (id, file) in files {
        let text = match nova_apps::scan::read_entry(&file) {
            Ok(text) => text,
            Err(e) => {
                problems.push(format!("can't read {}: {e}", file.display()));
                continue;
            }
        };
        match desktop_entry::parse(&id, &file, &text, context) {
            Ok(entry) => apps.push(entry),
            Err(Skipped::Invalid(why)) => problems.push(format!("skipping {}: {why}", file.display())),
            Err(Skipped::Hidden | Skipped::NotShown) => {}
        }
    }
    (apps, problems)
}

/// Starts the login apps on a thread of its own, unless `marker` says a
/// novad already did this session (it lives in `XDG_RUNTIME_DIR`, which
/// goes at logout), so a restarted novad doesn't start them twice.
pub fn start(marker: &Path, path_var: String, logger: Arc<Logger>) {
    let marker = marker.to_path_buf();
    // On a thread from the start: asking systemd may take a while.
    std::thread::spawn(move || {
        // A compositor that runs systemd's autostart itself (niri does) on a
        // system where Nova's unit isn't installed.
        if systemd_starts_them(&path_var, &logger) {
            logger.info(LOG_SOURCE, "systemd starts the login apps (xdg-desktop-autostart.target)");
            return;
        }
        match first_this_session(&marker) {
            Ok(true) => {}
            Ok(false) => {
                logger.info(LOG_SOURCE, "login apps already started this session");
                return;
            }
            // Started anyway: missing them is worse than a second start.
            Err(e) => logger.warn(LOG_SOURCE, &format!("can't write {}: {e}", marker.display())),
        }
        let locale = Locale::from_env();
        let desktops = nova_apps::current_desktops();
        let context = Context { locale: &locale, desktops: &desktops, path_var: &path_var, autostart: true };
        let (apps, problems) = entries(&dirs(), &context);
        for problem in problems {
            logger.warn(LOG_SOURCE, &problem);
        }
        let runner = Runner::detect(&path_var);
        for app in &apps {
            if let Err(e) = launch::launch(app, &runner, &path_var, &logger) {
                logger.warn(LOG_SOURCE, &format!("can't start {}: {e}", app.id));
            }
        }
        logger.info(LOG_SOURCE, &format!("started {} login apps", apps.len()));
    });
}

/// Whether the user's systemd runs `xdg-desktop-autostart.target`.
/// One that doesn't answer in time counts as no.
fn systemd_starts_them(path_var: &str, logger: &Logger) -> bool {
    let Some(systemctl) = nova_paths::find_program("systemctl", path_var) else { return false };
    let mut command = std::process::Command::new(systemctl);
    command.args(["--user", "show", "--property=ActiveState", "--value", SYSTEMD_TARGET]);
    match nova_process::child::stdout_within(command, SYSTEMCTL_TIMEOUT) {
        Ok(state) => matches!(String::from_utf8_lossy(&state).trim(), "active" | "activating" | "reloading"),
        Err(e) => {
            logger.warn(LOG_SOURCE, &format!("can't ask systemd about {SYSTEMD_TARGET}: {e}"));
            false
        }
    }
}

/// Notes in `marker` that the login apps are started: false when a novad
/// already did.
fn first_this_session(marker: &Path) -> std::io::Result<bool> {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(marker) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> nova_test_support::TempDir {
        nova_test_support::TempDir::new(&format!("autostart-{name}"))
    }

    fn write(dir: &Path, file: &str, extra: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(file), format!("[Desktop Entry]\nType=Application\nName={file}\nExec=true\n{extra}"))
            .unwrap();
    }

    fn ids(dirs: &[PathBuf]) -> Vec<String> {
        let locale = Locale::default();
        let desktops = vec!["niri".to_string()];
        let context = Context { locale: &locale, desktops: &desktops, path_var: "/bin:/usr/bin", autostart: true };
        entries(dirs, &context).0.into_iter().map(|entry| entry.id).collect()
    }

    #[test]
    fn the_users_entries_replace_the_systems_and_switches_count() {
        let root = temp("entries");
        let (user, system) = (root.join("user"), root.join("system"));
        // The user turned the system's one off with a copy.
        write(&system, "blueman.desktop", "");
        write(&user, "blueman.desktop", "Hidden=true\n");
        write(&system, "steam.desktop", "NoDisplay=true\n");
        write(&user, "discord.desktop", "");
        write(&user, "off.desktop", "X-GNOME-Autostart-enabled=false\n");
        write(&user, "gnome-only.desktop", "OnlyShowIn=GNOME;\n");
        write(&user, "not-niri.desktop", "NotShowIn=niri;\n");
        write(&user, "missing.desktop", "TryExec=surely-not-installed-anywhere\n");
        write(&user, "notes.txt", "");
        assert_eq!(ids(&[user, system]), ["discord.desktop", "steam.desktop"]);
    }

    #[test]
    fn a_restarted_novad_starts_none_twice() {
        let root = temp("marker");
        let marker = root.join("autostarted");
        assert!(first_this_session(&marker).unwrap());
        assert!(!first_this_session(&marker).unwrap());
        assert!(first_this_session(&root.join("gone/autostarted")).is_err());
    }
}
