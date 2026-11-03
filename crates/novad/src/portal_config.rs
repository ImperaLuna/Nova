//! Points xdg-desktop-portal's file chooser at Nova (`nova setup`, while
//! the `files` feature is on): the portal reads one config file per
//! session, the first of `<desktop>-portals.conf` then `portals.conf` in
//! the user's config folder, the system's config folders, then the data
//! folders. Setup writes the user's `<desktop>-portals.conf`, starting from
//! the file the portal reads now so its other choices (screen sharing,
//! secrets) stay, with `org.freedesktop.impl.portal.FileChooser=nova;`
//! ahead of what chose the file chooser before, which stays as the
//! fallback when Nova's backend isn't installed. The NixOS and
//! home-manager modules do the same through `xdg.portal.config`.

use crate::DEFAULT_DATA_DIRS;
use crate::apps::associations::{DEFAULT_CONFIG_DIRS, read, set_key};
use std::path::{Path, PathBuf};

/// The portal's folder name, in each config and data folder.
const PORTAL_DIR: &str = "xdg-desktop-portal";
const GENERIC: &str = "portals.conf";
const PREFERRED: &str = "preferred";
const FILE_CHOOSER: &str = "org.freedesktop.impl.portal.FileChooser";
/// Nova's backend, as `nova.portal` names it.
const NOVA: &str = "nova";

/// Makes Nova the file chooser on `desktop` (`niri`, `hyprland`) for the
/// user whose config folder is `config_home`, reading what the portal uses
/// now from this session's config and data folders. Returns the file
/// written.
pub fn prefer_nova_file_chooser(config_home: &Path, desktop: &str) -> Result<PathBuf, String> {
    let dirs = |var: &str, default: &str| {
        let value = nova_paths::path_var(var).unwrap_or_else(|| default.into());
        std::env::split_paths(&value).filter(|dir| dir.is_absolute()).collect::<Vec<_>>()
    };
    let config_dirs = dirs("XDG_CONFIG_DIRS", DEFAULT_CONFIG_DIRS);
    let data_dirs = dirs("XDG_DATA_DIRS", DEFAULT_DATA_DIRS);
    let system: Vec<PathBuf> = config_dirs.iter().chain(&data_dirs).cloned().collect();
    prefer_in(config_home, &system, desktop)
}

/// The same, with the system's config then data folders `system`.
fn prefer_in(config_home: &Path, system: &[PathBuf], desktop: &str) -> Result<PathBuf, String> {
    let desktop = desktop.to_lowercase();
    let own = format!("{desktop}-{GENERIC}");
    let user_dir = config_home.join(PORTAL_DIR);
    let target = user_dir.join(&own);
    let read_now = std::iter::once(user_dir.clone())
        .chain(system.iter().map(|dir| dir.join(PORTAL_DIR)))
        .flat_map(|dir| [dir.join(&own), dir.join(GENERIC)])
        .find_map(|path| read(&path).transpose().map(|text| (path, text)));
    let text = match read_now {
        Some((path, text)) => text.map_err(|e| format!("can't read {}: {e}", path.display()))?,
        None => String::new(),
    };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    set_key(&mut lines, PREFERRED, FILE_CHOOSER, |before| {
        std::iter::once(NOVA.to_string()).chain(before.into_iter().filter(|portal| portal != NOVA)).collect()
    });
    let written = lines.join("\n") + "\n";
    crate::state_file::write(&target, written.as_bytes())
        .map_err(|e| format!("can't write {}: {e}", target.display()))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    #[test]
    fn the_portals_current_choices_stay_with_nova_first_for_files() {
        let dir = TempDir::new("portal-config");
        let share = dir.join("share");
        std::fs::create_dir_all(share.join(PORTAL_DIR)).unwrap();
        std::fs::write(
            share.join(PORTAL_DIR).join("niri-portals.conf"),
            "[preferred]\ndefault=gnome;gtk;\norg.freedesktop.impl.portal.FileChooser=gtk;\n",
        )
        .unwrap();
        let config = dir.join("config");
        let written = prefer_in(&config, &[dir.join("etc"), share], "niri").unwrap();
        assert_eq!(written, config.join(PORTAL_DIR).join("niri-portals.conf"));
        let expected = "[preferred]\ndefault=gnome;gtk;\norg.freedesktop.impl.portal.FileChooser=nova;gtk;\n";
        assert_eq!(std::fs::read_to_string(&written).unwrap(), expected);
        // Again, from the user's own file now: nothing changes.
        prefer_in(&config, &[], "niri").unwrap();
        assert_eq!(std::fs::read_to_string(&written).unwrap(), expected);
    }

    #[test]
    fn with_no_portal_config_nova_is_the_only_choice() {
        let dir = TempDir::new("portal-config-none");
        let written = prefer_in(&dir.join("config"), &[dir.join("share")], "Hyprland").unwrap();
        assert!(written.ends_with("hyprland-portals.conf"));
        let text = std::fs::read_to_string(&written).unwrap();
        assert_eq!(text, "[preferred]\norg.freedesktop.impl.portal.FileChooser=nova;\n");
    }
}
