//! The active theme: the user picks one by name in `config.toml`, and its
//! colors come from `themes/<id>.toml`. Anything wrong with either falls back
//! to Carbonfox dark, which is compiled in so the fallback can't fail.
//!
//! ```toml
//! # ~/.config/nova/config.toml
//! [theme]
//! name = "carbonfox"
//! mode = "dark"
//! ```

use nova_protocol::{Palette, Theme, ThemeMode};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_THEME_ID: &str = "carbonfox";
const DEFAULT_THEME_FILE: &str = include_str!("../../../themes/carbonfox.toml");
const THEME_FILE_EXTENSION: &str = "toml";

/// The user's `config.toml`. Only the theme lives here so far.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    theme: ThemeChoice,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct ThemeChoice {
    name: String,
    mode: ThemeMode,
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self { name: DEFAULT_THEME_ID.into(), mode: ThemeMode::default() }
    }
}

/// `themes/<id>.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    dark: Palette,
    light: Option<Palette>,
}

/// The installed themes, else `./themes` for a checkout.
pub fn default_dir() -> PathBuf {
    nova_paths::installed_or_checkout("themes")
}

/// The theme `config` asks for, plus a message for each problem that made it
/// fall back (to log as warnings).
pub fn resolve(themes_dir: &Path, config: &Path) -> (Theme, Vec<String>) {
    let mut problems = Vec::new();
    let choice = read_config(config).unwrap_or_else(|problem| {
        problems.push(problem);
        ThemeChoice::default()
    });
    let (id, file) = read_theme(themes_dir, &choice.name).unwrap_or_else(|problem| {
        problems.push(format!("{problem}; using {DEFAULT_THEME_ID}"));
        (DEFAULT_THEME_ID.into(), default_theme_file())
    });
    let (mode, colors) = match (choice.mode, file.light) {
        (ThemeMode::Light, Some(light)) => (ThemeMode::Light, light),
        (ThemeMode::Light, None) => {
            problems.push(format!("theme {id} has no light colors; using dark"));
            (ThemeMode::Dark, file.dark)
        }
        (ThemeMode::Dark, _) => (ThemeMode::Dark, file.dark),
    };
    (Theme { id, name: file.name, mode, colors }, problems)
}

fn read_config(path: &Path) -> Result<ThemeChoice, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ThemeChoice::default()),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    let config: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(config.theme)
}

fn read_theme(dir: &Path, id: &str) -> Result<(String, ThemeFile), String> {
    // The id becomes a file name, so `../` and the like never reach the path.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!("theme name {id:?} may only use a-z, 0-9 and -"));
    }
    let path = dir.join(id).with_extension(THEME_FILE_EXTENSION);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("can't read theme {}: {e}", path.display()))?;
    let file = toml::from_str(&text).map_err(|e| format!("theme {}: {e}", path.display()))?;
    Ok((id.into(), file))
}

fn default_theme_file() -> ThemeFile {
    toml::from_str(DEFAULT_THEME_FILE).expect("themes/carbonfox.toml is valid (the tests load it)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::Color;

    /// WCAG 2 minimum for body text.
    const MIN_TEXT_CONTRAST: f64 = 4.5;

    fn themes_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes")
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("novad-theme-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn shipped_theme_ids() -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(themes_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == THEME_FILE_EXTENSION))
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        ids.sort();
        ids
    }

    fn contrast(a: &Color, b: &Color) -> f64 {
        fn luminance(color: &Color) -> f64 {
            let [red, green, blue] = color.rgb().map(|channel| {
                let value = f64::from(channel) / 255.0;
                if value <= 0.03928 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
            });
            0.2126 * red + 0.7152 * green + 0.0722 * blue
        }
        let (lighter, darker) = {
            let (x, y) = (luminance(a), luminance(b));
            (x.max(y), x.min(y))
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn contrast_matches_known_ratios() {
        let black: Color = "#000000".parse().unwrap();
        let white: Color = "#FFFFFF".parse().unwrap();
        assert!((contrast(&black, &white) - 21.0).abs() < 0.01);
        assert!((contrast(&white, &white) - 1.0).abs() < 0.01);
    }

    #[test]
    fn every_shipped_theme_loads_without_problems_and_is_readable() {
        let ids = shipped_theme_ids();
        assert!(ids.contains(&DEFAULT_THEME_ID.to_owned()));
        let scratch = Scratch::new("shipped");
        for id in ids {
            for mode in ["dark", "light"] {
                let config = scratch.write("config.toml", &format!("[theme]\nname = \"{id}\"\nmode = \"{mode}\"\n"));
                let (theme, problems) = resolve(&themes_dir(), &config);
                if mode == "light" && theme.mode == ThemeMode::Dark {
                    continue; // this theme has no light set
                }
                assert!(problems.is_empty(), "{id} {mode}: {problems:?}");
                assert_eq!(theme.id, id);
                let colors = &theme.colors;
                let text = contrast(&colors.fg, &colors.bg);
                let on_accent = contrast(&colors.on_accent, &colors.primary);
                assert!(text >= MIN_TEXT_CONTRAST, "{id} {mode}: fg on bg is {text:.2}:1");
                assert!(on_accent >= MIN_TEXT_CONTRAST, "{id} {mode}: onAccent on primary is {on_accent:.2}:1");
            }
        }
    }

    #[test]
    fn the_built_in_fallback_parses() {
        assert_eq!(default_theme_file().name, "Carbonfox");
    }

    #[test]
    fn no_config_means_carbonfox_dark() {
        let scratch = Scratch::new("no-config");
        let (theme, problems) = resolve(&themes_dir(), &scratch.0.join("config.toml"));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!((theme.id.as_str(), theme.mode), (DEFAULT_THEME_ID, ThemeMode::Dark));
    }

    #[test]
    fn unknown_or_unsafe_names_fall_back_with_a_reason() {
        let scratch = Scratch::new("bad-names");
        for (name, reason) in [("nope", "can't read theme"), ("../../etc/passwd", "may only use"), ("", "may only use")]
        {
            let config = scratch.write("config.toml", &format!("[theme]\nname = {name:?}\n"));
            let (theme, problems) = resolve(&themes_dir(), &config);
            assert_eq!(theme.id, DEFAULT_THEME_ID);
            assert!(problems.len() == 1 && problems[0].contains(reason), "{name:?}: {problems:?}");
        }
    }

    #[test]
    fn a_broken_config_falls_back_and_says_why() {
        let scratch = Scratch::new("bad-config");
        let config = scratch.write("config.toml", "[theme]\nname = \"carbonfox\"\nmode = \"sepia\"\n");
        let (theme, problems) = resolve(&themes_dir(), &config);
        assert_eq!(theme.id, DEFAULT_THEME_ID);
        assert!(problems.len() == 1 && problems[0].contains("sepia"), "{problems:?}");
    }

    #[test]
    fn light_without_a_light_set_uses_dark() {
        let scratch = Scratch::new("no-light");
        let dark_only = DEFAULT_THEME_FILE.split("\n[light]").next().unwrap().replace("Carbonfox", "Dark only");
        scratch.write("dark-only.toml", &dark_only);
        let config = scratch.write("config.toml", "[theme]\nname = \"dark-only\"\nmode = \"light\"\n");
        let (theme, problems) = resolve(&scratch.0, &config);
        assert_eq!((theme.name.as_str(), theme.mode), ("Dark only", ThemeMode::Dark));
        assert!(problems.len() == 1 && problems[0].contains("no light colors"), "{problems:?}");
    }

    #[test]
    fn themes_with_missing_or_extra_tokens_are_refused() {
        let scratch = Scratch::new("tokens");
        let config = scratch.write("config.toml", "[theme]\nname = \"custom\"\n");
        let missing = DEFAULT_THEME_FILE.lines().filter(|line| !line.starts_with("swatch8 ")).collect::<Vec<_>>();
        let extra = DEFAULT_THEME_FILE.replace("[dark]\n", "[dark]\nsurface = \"#000000\"\n");
        for (text, token) in [(missing.join("\n"), "swatch8"), (extra, "surface")] {
            scratch.write("custom.toml", &text);
            let (theme, problems) = resolve(&scratch.0, &config);
            assert_eq!(theme.id, DEFAULT_THEME_ID);
            assert!(problems.len() == 1 && problems[0].contains(token), "{token}: {problems:?}");
        }
    }

    /// Tokens.qml declares a property for every palette token, so the UI can
    /// apply each color it gets from novad.
    #[test]
    fn tokens_qml_declares_every_palette_token() {
        let tokens_qml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/theme/Tokens.qml");
        let tokens_qml = std::fs::read_to_string(tokens_qml).unwrap();
        let palette = serde_json::to_value(default_theme_file().dark).unwrap();
        for token in palette.as_object().unwrap().keys() {
            let declared = format!("property color {token}\n");
            assert!(tokens_qml.contains(&declared), "Tokens.qml has no `{}`", declared.trim());
        }
    }
}
