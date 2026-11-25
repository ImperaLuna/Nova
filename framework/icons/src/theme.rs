use super::ini::Ini;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const HICOLOR: &str = "hicolor";
const GTK_DEFAULT: &str = "Adwaita";
const PLATFORM_THEME_VAR: &str = "QT_QPA_PLATFORMTHEME";
const INDEX_FILE: &str = "index.theme";
const THEME_GROUP: &str = "Icon Theme";
const MAX_THEMES: usize = 16;
const DEFAULT_THRESHOLD: u32 = 2;

struct Setting {
    file: &'static str,
    group: &'static str,
    key: &'static str,
}

const QT6CT: Setting = Setting { file: "qt6ct/qt6ct.conf", group: "Appearance", key: "icon_theme" };
const QT5CT: Setting = Setting { file: "qt5ct/qt5ct.conf", group: "Appearance", key: "icon_theme" };
const KDE: Setting = Setting { file: "kdeglobals", group: "Icons", key: "Theme" };
const GTK4: Setting = Setting { file: "gtk-4.0/settings.ini", group: "Settings", key: "gtk-icon-theme-name" };
const GTK3: Setting = Setting { file: "gtk-3.0/settings.ini", group: "Settings", key: "gtk-icon-theme-name" };

pub fn current_name(var: impl Fn(&str) -> Option<String>, config_home: Option<&Path>, base_dirs: &[PathBuf]) -> String {
    let installed = |name: &str| base_dirs.iter().any(|base| base.join(name).join(INDEX_FILE).is_file());
    let platform = var(PLATFORM_THEME_VAR).unwrap_or_default();
    let settings: [&Setting; 5] = match platform.as_str() {
        "qt6ct" | "qt5ct" => [&QT6CT, &QT5CT, &KDE, &GTK4, &GTK3],
        "kde" => [&KDE, &QT6CT, &QT5CT, &GTK4, &GTK3],
        _ => [&GTK4, &GTK3, &QT6CT, &QT5CT, &KDE],
    };
    config_home
        .and_then(|config| {
            settings.iter().find_map(|setting| {
                let ini = Ini::read(&config.join(setting.file))?;
                Some(ini.get(setting.group, setting.key)?.to_string()).filter(|name| is_name(name) && installed(name))
            })
        })
        .or_else(|| installed(GTK_DEFAULT).then(|| GTK_DEFAULT.to_string()))
        .unwrap_or_else(|| HICOLOR.to_string())
}

pub fn is_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\0'])
}

#[derive(Debug, PartialEq)]
pub struct IconDir {
    pub path: PathBuf,
    pub generic: bool,
}

const EXACT_CONTEXTS: [&str; 2] = ["Applications", "MimeTypes"];

pub fn icon_dirs(theme: &str, base_dirs: &[PathBuf], size: u32) -> Vec<IconDir> {
    let mut dirs = Vec::new();
    for name in chain(theme, base_dirs) {
        let Some(index) = index(&name, base_dirs) else { continue };
        let mut subdirs: Vec<(u32, &str)> = index
            .list(THEME_GROUP, "Directories")
            .into_iter()
            .chain(index.list(THEME_GROUP, "ScaledDirectories"))
            .filter(|subdir| Path::new(subdir).is_relative() && !subdir.split('/').any(|part| part == ".."))
            .map(|subdir| (SubDir::read(&index, subdir).distance(size), subdir))
            .collect();
        subdirs.sort_by_key(|(distance, _)| *distance);
        let mut seen = BTreeSet::new();
        for (_, subdir) in subdirs {
            if !seen.insert(subdir) {
                continue;
            }
            let generic = !index.get(subdir, "Context").is_some_and(|context| EXACT_CONTEXTS.contains(&context));
            dirs.extend(
                base_dirs
                    .iter()
                    .map(|base| base.join(&name).join(subdir))
                    .filter(|dir| dir.is_dir())
                    .map(|path| IconDir { path, generic }),
            );
        }
    }
    dirs
}

fn chain(theme: &str, base_dirs: &[PathBuf]) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut next = vec![theme.to_string()];
    while let Some(name) = next.pop() {
        if chain.len() >= MAX_THEMES || name == HICOLOR || chain.contains(&name) || !is_name(&name) {
            continue;
        }
        if let Some(index) = index(&name, base_dirs) {
            next.extend(index.list(THEME_GROUP, "Inherits").into_iter().rev().map(str::to_string));
        }
        chain.push(name);
    }
    chain.push(HICOLOR.to_string());
    chain
}

fn index(theme: &str, base_dirs: &[PathBuf]) -> Option<Ini> {
    base_dirs.iter().find_map(|base| Ini::read(&base.join(theme).join(INDEX_FILE)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

#[derive(Debug, PartialEq)]
struct SubDir {
    kind: Kind,
    size: u32,
    min: u32,
    max: u32,
    threshold: u32,
}

impl SubDir {
    fn read(index: &Ini, subdir: &str) -> Self {
        let number = |key: &str| index.get(subdir, key).and_then(|value| value.parse::<u32>().ok());
        let scale = number("Scale").unwrap_or(1).max(1);
        let size = number("Size").unwrap_or(0);
        let kind = match index.get(subdir, "Type") {
            Some("Fixed") => Kind::Fixed,
            Some("Scalable") => Kind::Scalable,
            _ => Kind::Threshold,
        };
        Self {
            kind,
            size: size.saturating_mul(scale),
            min: number("MinSize").unwrap_or(size).saturating_mul(scale),
            max: number("MaxSize").unwrap_or(size).saturating_mul(scale),
            threshold: number("Threshold").unwrap_or(DEFAULT_THRESHOLD).saturating_mul(scale),
        }
    }

    fn distance(&self, size: u32) -> u32 {
        let (low, high) = match self.kind {
            Kind::Fixed => (self.size, self.size),
            Kind::Scalable => (self.min, self.max),
            Kind::Threshold => (self.size.saturating_sub(self.threshold), self.size.saturating_add(self.threshold)),
        };
        if size < low { low - size } else { size.saturating_sub(high) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn theme(base: &Path, name: &str, index: &str, subdirs: &[&str]) {
        let dir = base.join(name);
        for subdir in subdirs {
            std::fs::create_dir_all(dir.join(subdir)).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(INDEX_FILE), index).unwrap();
    }

    #[test]
    fn subdirs_measure_their_distance_like_the_spec() {
        let index = Ini::parse(
            "[48x48/apps]\nSize=48\nType=Fixed\n[scalable/apps]\nSize=64\nMinSize=16\nMaxSize=256\nType=Scalable\n[32x32/apps]\nSize=32\n[64x64@2/apps]\nSize=64\nScale=2\nType=Fixed\n",
        );
        assert_eq!(SubDir::read(&index, "48x48/apps").distance(128), 80);
        assert_eq!(SubDir::read(&index, "48x48/apps").distance(48), 0);
        assert_eq!(SubDir::read(&index, "scalable/apps").distance(128), 0);
        assert_eq!(SubDir::read(&index, "scalable/apps").distance(512), 256);
        assert_eq!(SubDir::read(&index, "32x32/apps").distance(34), 0);
        assert_eq!(SubDir::read(&index, "32x32/apps").distance(40), 6);
        assert_eq!(SubDir::read(&index, "64x64@2/apps").distance(128), 0);
    }

    #[test]
    fn folders_come_closest_size_first_through_the_inherited_themes() {
        let dir = TempDir::new("icon-dirs");
        let (user, system) = (dir.path().join("user"), dir.path().join("system"));
        theme(
            &system,
            "Papirus-Dark",
            "[Icon Theme]\nInherits=Papirus,hicolor\nDirectories=16x16/apps\n[16x16/apps]\nSize=16\nType=Fixed\n",
            &["16x16/apps"],
        );
        theme(
            &system,
            "Papirus",
            "[Icon Theme]\nInherits=Papirus-Dark\nDirectories=16x16/apps,128x128/apps,../escape,/tmp\n[16x16/apps]\nSize=16\nType=Fixed\n[128x128/apps]\nSize=128\nType=Fixed\n",
            &["16x16/apps", "128x128/apps"],
        );
        theme(
            &system,
            HICOLOR,
            "[Icon Theme]\nDirectories=48x48/apps,scalable/apps\n[48x48/apps]\nSize=48\nType=Fixed\n[scalable/apps]\nSize=128\nType=Scalable\nMinSize=1\nMaxSize=512\n",
            &["48x48/apps", "scalable/apps"],
        );
        std::fs::create_dir_all(user.join("hicolor/scalable/apps")).unwrap();

        let dirs = icon_dirs("Papirus-Dark", &[user.clone(), system.clone()], 128);
        assert_eq!(
            dirs.into_iter().map(|dir| dir.path).collect::<Vec<_>>(),
            [
                system.join("Papirus-Dark/16x16/apps"),
                system.join("Papirus/128x128/apps"),
                system.join("Papirus/16x16/apps"),
                user.join("hicolor/scalable/apps"),
                system.join("hicolor/scalable/apps"),
                system.join("hicolor/48x48/apps"),
            ]
        );
    }

    #[test]
    fn the_users_theme_comes_from_their_desktops_settings() {
        let dir = TempDir::new("icon-theme-name");
        let (config, icons) = (dir.path().join("config"), dir.path().join("icons"));
        for name in ["Papirus-Dark", "breeze"] {
            theme(&icons, name, "[Icon Theme]\n", &[]);
        }
        std::fs::create_dir_all(config.join("gtk-3.0")).unwrap();
        std::fs::write(config.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-icon-theme-name=Papirus-Dark\n").unwrap();
        std::fs::write(config.join("kdeglobals"), "[Icons]\nTheme=breeze\n").unwrap();
        let bases = [icons.clone()];
        let name = |vars: &[(&str, &str)]| {
            let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            current_name(
                |var| vars.iter().find(|(name, _)| name == var).map(|(_, value)| value.clone()),
                Some(&config),
                &bases,
            )
        };
        assert_eq!(name(&[]), "Papirus-Dark");
        assert_eq!(name(&[(PLATFORM_THEME_VAR, "kde")]), "breeze");
        std::fs::write(config.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-icon-theme-name=Gone\n").unwrap();
        assert_eq!(name(&[]), "breeze");
        assert_eq!(current_name(|_| None, None, &bases), HICOLOR);
        theme(&icons, GTK_DEFAULT, "[Icon Theme]\n", &[]);
        assert_eq!(current_name(|_| None, None, &bases), GTK_DEFAULT, "GTK's own default");
    }
}
