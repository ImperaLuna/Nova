//! App icons as files, so the UI only ever loads an image and never has Qt
//! search an icon theme on its GUI thread (33-154 ms for each name the
//! first time, which every screen's animations wait on; IMP-158).
//!
//! A name is looked for as the freedesktop.org Icon Theme Specification
//! says: in the user's theme (`theme::current_name`), the themes it
//! inherits and hicolor, in the folders closest to `LOOKUP_SIZE` first,
//! then loose in the base and pixmaps folders. Every answer is kept until
//! `forget`, which the launcher calls whenever the installed apps change,
//! since apps bring their icons with them. The theme's name is read once at
//! start and the UI gets it too (`QS_ICON_THEME`), so what Qt still looks up
//! itself (tray menus' icons) comes from the same theme.

mod ini;
mod theme;

#[cfg(test)]
pub use theme::HICOLOR;
pub use theme::QUICKSHELL_VAR;

use crate::lock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The size icons are looked for at, in pixels: the largest Nova draws an
/// app's icon (the launcher's tiles) at twice its size, rounded up to the
/// size themes ship. Smaller ones scale down from it.
const LOOKUP_SIZE: u32 = 128;
/// Image types in the order they're preferred, as the spec has them.
const EXTENSIONS: [&str; 3] = ["png", "svg", "xpm"];
/// Names whose answer is kept at most; past it, they're all forgotten.
/// Socket clients choose names, so this bounds what they can make novad
/// keep. Far more than the apps, notifications and streams of a session.
const MAX_KEPT: usize = 4096;
/// The longest name looked up, like a file name's limit.
const MAX_NAME_BYTES: usize = 255;

pub struct Icons {
    theme: String,
    /// `~/.icons`, then each data folder's `icons/`.
    base_dirs: Vec<PathBuf>,
    /// Where loose icons are, searched last: the base folders, then each
    /// data folder's `pixmaps/`.
    loose_dirs: Vec<PathBuf>,
    kept: Mutex<Kept>,
}

#[derive(Default)]
struct Kept {
    /// The theme folders to look in, best first; read again after `forget`.
    dirs: Option<Arc<Vec<theme::IconDir>>>,
    /// Each name looked up, and its file if it has one.
    found: HashMap<String, Option<String>>,
    /// Counts `forget`s, so an answer from before one isn't kept after it.
    generation: u64,
}

impl Icons {
    /// For the session novad runs in.
    pub fn for_session() -> Self {
        let (data_dirs, home) = (crate::data_dirs(), nova_paths::home());
        let config_home = nova_paths::config_home();
        let theme = theme::current_name(
            |name| std::env::var(name).ok(),
            config_home.as_deref(),
            &base_dirs(home.as_deref(), &data_dirs),
        );
        Self::new(theme, &data_dirs, home.as_deref())
    }

    /// Icons in theme `theme`, from the data folders `data_dirs` (in
    /// order) and the user's `home`.
    pub fn new(theme: String, data_dirs: &[PathBuf], home: Option<&Path>) -> Self {
        let base_dirs = base_dirs(home, data_dirs);
        let loose_dirs = base_dirs.iter().cloned().chain(data_dirs.iter().map(|dir| dir.join("pixmaps"))).collect();
        Self { theme, base_dirs, loose_dirs, kept: Mutex::default() }
    }

    /// The icon theme's name, for the UI's `QS_ICON_THEME`.
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// The file for icon `name` (an icon name, never a path), or `None`
    /// when no theme has it.
    pub fn find(&self, name: &str) -> Option<String> {
        // Desktop entries sometimes give the file's name.
        let name =
            EXTENSIONS.iter().find_map(|extension| name.strip_suffix(extension)?.strip_suffix('.')).unwrap_or(name);
        if name.len() > MAX_NAME_BYTES || !theme::is_name(name) {
            return None;
        }
        let (dirs, generation) = {
            let mut kept = lock(&self.kept);
            if let Some(found) = kept.found.get(name) {
                return found.clone();
            }
            let dirs =
                kept.dirs.get_or_insert_with(|| Arc::new(theme::icon_dirs(&self.theme, &self.base_dirs, LOOKUP_SIZE)));
            (Arc::clone(dirs), kept.generation)
        };
        // Outside the lock: a miss looks in every folder. Then, as Qt does,
        // shorter names in the folders that allow it.
        let found =
            dirs.iter().map(|dir| &dir.path).chain(&self.loose_dirs).find_map(|dir| file_in(dir, name)).or_else(|| {
                shorter_names(name).find_map(|shorter| {
                    dirs.iter().filter(|dir| dir.generic).find_map(|dir| file_in(&dir.path, shorter))
                })
            });
        let mut kept = lock(&self.kept);
        if kept.generation == generation {
            if kept.found.len() >= MAX_KEPT {
                kept.found.clear();
            }
            kept.found.insert(name.to_string(), found.clone());
        }
        found
    }

    /// The file for a desktop entry's `Icon`: an absolute path to an
    /// image as it is, when it's there, else an icon name's file (`find`).
    pub fn entry_icon(&self, icon: &str) -> Option<String> {
        let path = Path::new(icon);
        if path.is_absolute() {
            return path.is_file().then(|| icon.to_string());
        }
        self.find(icon)
    }

    /// Forgets every answer and the theme's folders, for when icons may
    /// have been installed or removed.
    pub fn forget(&self) {
        let mut kept = lock(&self.kept);
        kept.dirs = None;
        kept.found.clear();
        kept.generation += 1;
    }
}

/// The spec's base folders: `~/.icons`, then each data folder's `icons/`.
fn base_dirs(home: Option<&Path>, data_dirs: &[PathBuf]) -> Vec<PathBuf> {
    home.map(|home| home.join(".icons")).into_iter().chain(data_dirs.iter().map(|dir| dir.join("icons"))).collect()
}

/// `name` without its last dash-separated parts, the longest first:
/// `dialog-information`, then `dialog`, for `dialog-information-symbolic`.
fn shorter_names(name: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(name.rsplit_once('-').map(|(shorter, _)| shorter), |name| {
        name.rsplit_once('-').map(|(shorter, _)| shorter)
    })
    .filter(|shorter| !shorter.is_empty())
}

/// `name`'s image in `dir`, of the first type there is.
fn file_in(dir: &Path, name: &str) -> Option<String> {
    EXTENSIONS
        .iter()
        .map(|extension| dir.join(format!("{name}.{extension}")))
        .find(|file| file.is_file())
        .map(|file| file.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    /// A data folder with a Papirus theme that inherits hicolor, the user's
    /// theme `Icons` is made with.
    struct Fixture {
        dir: TempDir,
        icons: Icons,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = TempDir::new("icons");
            let share = dir.path().join("share");
            write(
                &share.join("icons/Papirus/index.theme"),
                "[Icon Theme]\nInherits=hicolor\nDirectories=64x64/apps,64x64/status\n[64x64/apps]\nSize=64\nType=Fixed\nContext=Applications\n[64x64/status]\nSize=64\nType=Fixed\nContext=Status\n",
            );
            write(
                &share.join("icons/hicolor/index.theme"),
                "[Icon Theme]\nDirectories=48x48/apps,scalable/apps\n[48x48/apps]\nSize=48\nType=Fixed\n[scalable/apps]\nSize=128\nType=Scalable\nMinSize=1\nMaxSize=512\n",
            );
            let icons = Icons::new("Papirus".into(), &[share], Some(&dir.path().join("home")));
            Self { dir, icons }
        }

        fn share(&self, path: &str) -> PathBuf {
            self.dir.path().join("share").join(path)
        }
    }

    fn write(file: &Path, text: &str) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    fn path(file: PathBuf) -> Option<String> {
        Some(file.display().to_string())
    }

    #[test]
    fn names_are_found_in_the_theme_then_inherited_then_loose() {
        let fixture = Fixture::new();
        for file in [
            "icons/Papirus/64x64/apps/firefox.svg",
            "icons/hicolor/48x48/apps/firefox.png",
            "icons/hicolor/48x48/apps/steam.png",
            "icons/hicolor/scalable/apps/steam.svg",
            "pixmaps/xterm.xpm",
        ] {
            write(&fixture.share(file), "");
        }
        let icons = &fixture.icons;
        assert_eq!(icons.find("firefox"), path(fixture.share("icons/Papirus/64x64/apps/firefox.svg")));
        // hicolor's scalable folder is closer to the lookup size.
        assert_eq!(icons.find("steam"), path(fixture.share("icons/hicolor/scalable/apps/steam.svg")));
        assert_eq!(icons.find("xterm"), path(fixture.share("pixmaps/xterm.xpm")));
        assert_eq!(icons.find("xterm.xpm"), path(fixture.share("pixmaps/xterm.xpm")));
        assert_eq!(icons.find("nothing"), None);
    }

    #[test]
    fn shorter_names_match_only_generic_icons() {
        let fixture = Fixture::new();
        write(&fixture.share("icons/Papirus/64x64/status/dialog-information.svg"), "");
        write(&fixture.share("icons/Papirus/64x64/apps/google.svg"), "");
        let icons = &fixture.icons;
        assert_eq!(
            icons.find("dialog-information-symbolic"),
            path(fixture.share("icons/Papirus/64x64/status/dialog-information.svg"))
        );
        assert_eq!(icons.find("google-chrome"), None, "an app's icon only by its own name");
        assert_eq!(shorter_names("a-b-c").collect::<Vec<_>>(), ["a-b", "a"]);
        assert_eq!(shorter_names("-a").count(), 0);
    }

    #[test]
    fn paths_are_never_names() {
        let fixture = Fixture::new();
        write(&fixture.share("icons/Papirus/64x64/apps/firefox.png"), "");
        write(&fixture.share("icons/Papirus/firefox.png"), "");
        let icons = &fixture.icons;
        for name in ["", "../firefox", "64x64/apps/firefox", ".hidden", "/etc/passwd"] {
            assert_eq!(icons.find(name), None, "{name:?}");
        }
        assert_eq!(icons.find(&"a".repeat(MAX_NAME_BYTES + 1)), None);
        // A desktop entry may give a file instead.
        let file = fixture.share("icons/Papirus/firefox.png");
        assert_eq!(icons.entry_icon(&file.display().to_string()), path(file.clone()));
        assert_eq!(icons.entry_icon(&fixture.share("gone.png").display().to_string()), None);
        assert_eq!(icons.entry_icon("firefox"), path(fixture.share("icons/Papirus/64x64/apps/firefox.png")));
    }

    #[test]
    fn answers_are_kept_until_forgotten() {
        let fixture = Fixture::new();
        let icons = &fixture.icons;
        assert_eq!(icons.find("signal"), None);
        // Installed with a folder the theme didn't have before.
        let file = fixture.share("icons/hicolor/48x48/apps/signal.png");
        write(&file, "");
        assert_eq!(icons.find("signal"), None, "kept");
        icons.forget();
        assert_eq!(icons.find("signal"), path(file));
    }
}
