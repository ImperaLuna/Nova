//! Where Nova keeps things, and where it finds the user's folders and
//! programs. novad and the `nova` CLI both resolve paths here, so the two
//! always agree. The XDG data folders apps, icons and file types are
//! looked up in ([`data_dirs`]) are here too.
//!
//! `ui/daemon/Daemon.qml` derives the socket path on its own (QML can't call
//! Rust); keep it in step with [`socket`].

pub mod reference;

use std::path::{Path, PathBuf};

/// `$XDG_RUNTIME_DIR/nova/novad.sock`, or `/tmp/nova-<uid>/nova/novad.sock`
/// without it.
pub fn socket() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/nova-{}", current_uid())));
    base.join("nova").join("novad.sock")
}

/// `$XDG_CONFIG_HOME/nova`, else `~/.config/nova`.
pub fn config_dir() -> PathBuf {
    in_or_here(config_home()).join("nova")
}

/// Where Nova looks for its config file `name`, in order: the user's
/// ([`config_dir`]), then the system's in each `$XDG_CONFIG_DIRS` folder
/// (`/etc/xdg` when unset), where the NixOS module puts features.toml. The
/// first that exists is the one read.
pub fn config_files(name: &str) -> Vec<PathBuf> {
    let system = path_var("XDG_CONFIG_DIRS").unwrap_or_else(|| PathBuf::from("/etc/xdg"));
    std::iter::once(config_dir().join(name))
        .chain(std::env::split_paths(&system).filter(|dir| dir.is_absolute()).map(|dir| dir.join("nova").join(name)))
        .collect()
}

/// `$XDG_STATE_HOME/nova`, else `~/.local/state/nova`. Logs live here.
pub fn state_dir() -> PathBuf {
    in_or_here(xdg_home("XDG_STATE_HOME", ".local/state")).join("nova")
}

/// `$HOME`, unless it's unset or empty.
pub fn home() -> Option<PathBuf> {
    path_var("HOME")
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
pub fn config_home() -> Option<PathBuf> {
    xdg_home("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_DATA_HOME`, else `~/.local/share`.
pub fn data_home() -> Option<PathBuf> {
    xdg_home("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_CACHE_HOME`, else `~/.cache`.
pub fn cache_home() -> Option<PathBuf> {
    xdg_home("XDG_CACHE_HOME", ".cache")
}

/// The path (or `:`-separated path list) in the environment variable
/// `name`, unless it's unset or empty (an empty XDG variable counts as
/// unset, per the spec).
pub fn path_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// `<prefix>/share/nova/<name>` next to the running binary (the Nix and
/// distro package layout) when it exists, else `./<name>` for a checkout.
pub fn installed_or_checkout(name: &str) -> PathBuf {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(|prefix| prefix.join("share/nova").join(name)));
    match installed {
        Some(dir) if dir.is_dir() => dir,
        _ => PathBuf::from(name),
    }
}

/// One of the user's folders from `user-dirs.dirs` (`xdg-user-dir`):
/// `key` is its name there without `XDG_` and `_DIR` (`PICTURES`), and
/// `home_fallback` the folder under `$HOME` when it isn't set.
pub fn user_dir(key: &str, home_fallback: &str) -> PathBuf {
    let home = in_or_here(home());
    let file = in_or_here(config_home()).join("user-dirs.dirs");
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| parse_user_dir(&text, key, &home))
        .unwrap_or_else(|| home.join(home_fallback))
}

/// `key`'s folder in a `user-dirs.dirs` file's text: lines like
/// `XDG_PICTURES_DIR="$HOME/Pictures"`, where only `$HOME/` is expanded.
fn parse_user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let name = format!("XDG_{key}_DIR=");
    let value = text.lines().find_map(|line| line.trim().strip_prefix(&name))?;
    let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
    let dir = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // A relative path, or `$HOME` alone, means the folder isn't set up.
    (dir.is_absolute() && dir != home).then_some(dir)
}

fn xdg_home(var: &str, home_fallback: &str) -> Option<PathBuf> {
    path_var(var).or_else(|| home().map(|home| home.join(home_fallback)))
}

/// `dir`, or the current folder without a home to put things in.
fn in_or_here(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| PathBuf::from("."))
}

fn current_uid() -> String {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid().to_string()).unwrap_or_else(|_| "unknown".to_owned())
}

/// novad's current log inside a log dir (normally [`state_dir`]).
pub fn log_file(log_dir: &Path) -> PathBuf {
    log_dir.join(LOG_FILE_NAME)
}

/// The log of an earlier novad run: 1 is the run before the current one.
pub fn previous_log_file(log_dir: &Path, runs_ago: usize) -> PathBuf {
    log_dir.join(format!("{LOG_FILE_NAME}.{runs_ago}"))
}

/// Saved log tails from crashes of the UI or novad, one file each, named by
/// time so that sorting by name sorts by age.
pub fn crash_dir(log_dir: &Path) -> PathBuf {
    log_dir.join("crashes")
}

const LOG_FILE_NAME: &str = "nova.log";

/// Where `program` runs from: itself when it's a path, else the first
/// executable file by that name in `path_var` (a `PATH`-style list). A file
/// without an executable bit doesn't count, as it wouldn't for `execvp`.
pub fn find_program(program: &str, path_var: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return is_executable(&path).then_some(path);
    }
    std::env::split_paths(path_var).map(|dir| dir.join(program)).find(|path| is_executable(path))
}

/// Whether `path` is a file with an executable bit.
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// The spec's default for an unset `XDG_DATA_DIRS`.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// The XDG data folders (the user's first), in the order they're searched
/// for apps and icons, plus the folders packages install into, which a
/// systemd user service's `XDG_DATA_DIRS` often lacks.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(data_home());
    let system = path_var("XDG_DATA_DIRS").unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
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
pub fn package_prefixes() -> Vec<PathBuf> {
    let home = home();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_expand_home_and_skip_unset_ones() {
        let home = Path::new("/home/me");
        let text = "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\nXDG_VIDEOS_DIR=\"/data/videos\"\nXDG_MUSIC_DIR=\"$HOME/\"\n";
        assert_eq!(parse_user_dir(text, "PICTURES", home), Some(PathBuf::from("/home/me/Bilder")));
        assert_eq!(parse_user_dir(text, "VIDEOS", home), Some(PathBuf::from("/data/videos")));
        assert_eq!(parse_user_dir(text, "MUSIC", home), None, "$HOME itself means unset");
        assert_eq!(parse_user_dir(text, "TEMPLATES", home), None);
    }

    #[test]
    fn a_program_on_path_needs_its_executable_bit() {
        use nova_test_support::{TempDir, write_program};
        let (first, second) = (TempDir::new("plain"), TempDir::new("programs"));
        std::fs::write(first.join("tool"), "").unwrap();
        write_program(second.path(), "tool", "exit 0");
        std::fs::create_dir(first.join("folder")).unwrap();
        let path_var = std::env::join_paths([first.path(), second.path()]).unwrap().into_string().unwrap();

        assert_eq!(find_program("tool", &path_var), Some(second.join("tool")), "skips the non-executable one");
        assert_eq!(find_program("folder", &path_var), None, "a folder isn't a program");
        let plain = first.join("tool");
        assert_eq!(find_program(plain.to_str().unwrap(), ""), None, "a path needs the bit too");
        assert_eq!(find_program("missing", &path_var), None);
    }
}
