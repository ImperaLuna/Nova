pub mod reference;

use std::path::{Path, PathBuf};

pub fn socket() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/nova-{}", current_uid())));
    base.join("nova").join("novad.sock")
}

pub fn config_dir() -> PathBuf {
    in_or_here(config_home()).join("nova")
}

pub fn config_files(name: &str) -> Vec<PathBuf> {
    let system = path_var("XDG_CONFIG_DIRS").unwrap_or_else(|| PathBuf::from("/etc/xdg"));
    std::iter::once(config_dir().join(name))
        .chain(std::env::split_paths(&system).filter(|dir| dir.is_absolute()).map(|dir| dir.join("nova").join(name)))
        .collect()
}

pub fn state_dir() -> PathBuf {
    in_or_here(xdg_home("XDG_STATE_HOME", ".local/state")).join("nova")
}

pub fn home() -> Option<PathBuf> {
    path_var("HOME")
}

pub fn config_home() -> Option<PathBuf> {
    xdg_home("XDG_CONFIG_HOME", ".config")
}

pub fn data_home() -> Option<PathBuf> {
    xdg_home("XDG_DATA_HOME", ".local/share")
}

pub fn cache_home() -> Option<PathBuf> {
    xdg_home("XDG_CACHE_HOME", ".cache")
}

pub fn path_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub fn installed_or_checkout(name: &str) -> PathBuf {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(|prefix| prefix.join("share/nova").join(name)));
    match installed {
        Some(dir) if dir.is_dir() => dir,
        _ => PathBuf::from(name),
    }
}

pub fn user_dir(key: &str, home_fallback: &str) -> PathBuf {
    let home = in_or_here(home());
    let file = in_or_here(config_home()).join("user-dirs.dirs");
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| parse_user_dir(&text, key, &home))
        .unwrap_or_else(|| home.join(home_fallback))
}

fn parse_user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let name = format!("XDG_{key}_DIR=");
    let value = text.lines().find_map(|line| line.trim().strip_prefix(&name))?;
    let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
    let dir = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    (dir.is_absolute() && dir != home).then_some(dir)
}

fn xdg_home(var: &str, home_fallback: &str) -> Option<PathBuf> {
    path_var(var).or_else(|| home().map(|home| home.join(home_fallback)))
}

fn in_or_here(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| PathBuf::from("."))
}

fn current_uid() -> String {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid().to_string()).unwrap_or_else(|_| "unknown".to_owned())
}

pub fn log_file(log_dir: &Path) -> PathBuf {
    log_dir.join(LOG_FILE_NAME)
}

pub fn previous_log_file(log_dir: &Path, runs_ago: usize) -> PathBuf {
    log_dir.join(format!("{LOG_FILE_NAME}.{runs_ago}"))
}

pub fn crash_dir(log_dir: &Path) -> PathBuf {
    log_dir.join("crashes")
}

const LOG_FILE_NAME: &str = "nova.log";

pub fn find_program(program: &str, path_var: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return is_executable(&path).then_some(path);
    }
    std::env::split_paths(path_var).map(|dir| dir.join(program)).find(|path| is_executable(path))
}

pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(data_home());
    let system = path_var("XDG_DATA_DIRS").unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    dirs.extend(std::env::split_paths(&system));
    dirs.extend(package_prefixes().into_iter().map(|prefix| prefix.join("share")));
    let mut seen = std::collections::BTreeSet::new();
    dirs.retain(|dir| dir.is_absolute() && seen.insert(dir.clone()));
    dirs
}

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
