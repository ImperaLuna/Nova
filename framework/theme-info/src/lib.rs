//! A theme as the `theme` event carries it to the UI ([`Theme`]): plain
//! data, so api crates and fronts can name it without the code that reads
//! theme files (`nova-theme`). A [`Color`] checks its `#RRGGBB` form when
//! it is read, which is part of the wire format.

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// The font families Nova's look is drawn in, the same in every theme:
/// running text, numbers, and display text. The look tokens name them
/// (`nova-look`, `ui/theme/Tokens.qml`), Nova's Nix modules install them
/// (`packaging/nix/fonts.nix`) and `nova doctor` checks that each is there.
pub const FONT_SANS: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";
pub const FONT_DISPLAY: &str = "Instrument Serif";
pub const FONTS: [&str; 3] = [FONT_SANS, FONT_MONO, FONT_DISPLAY];

/// The topic a client subscribes to for [`Event::Theme`].
pub const TOPIC: Topic = Topic::named("theme");

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The active theme's colors, sent right after subscribing.
    Theme(Theme),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// The theme file's name without `.toml`, e.g. `carbonfox`.
    pub id: String,
    /// Display name, e.g. `Carbonfox`.
    pub name: String,
    pub mode: ThemeMode,
    pub colors: Palette,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

/// Every color token a theme sets, by the name `ui/theme/Tokens.qml` exposes
/// it under. Theme files must set all of them and nothing else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Palette {
    pub bg: Color,
    pub bg_alt: Color,
    /// Behind everything when nothing else is drawn, like the lock screen.
    pub backdrop: Color,
    /// Glass: laid over the compositor's blur, translucent (Tokens sets how
    /// much).
    pub glass_tint: Color,
    /// Text and icons. Tokens derives the dimmer levels from it.
    pub fg: Color,
    pub primary: Color,
    pub secondary: Color,
    pub on_accent: Color,
    pub info: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub swatch1: Color,
    pub swatch2: Color,
    pub swatch3: Color,
    pub swatch4: Color,
    pub swatch5: Color,
    pub swatch6: Color,
    pub swatch7: Color,
    pub swatch8: Color,
}

/// An opaque `#RRGGBB` color. Anything else is refused when parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color([u8; 3]);

impl Color {
    pub fn rgb(&self) -> [u8; 3] {
        self.0
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("{text:?} isn't a #RRGGBB color");
        let hex = text.strip_prefix('#').filter(|hex| hex.len() == 6 && hex.is_ascii()).ok_or_else(invalid)?;
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).map_err(|_| invalid());
        Ok(Self([channel(0)?, channel(2)?, channel(4)?]))
    }
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Color> for String {
    fn from(color: Color) -> Self {
        let [red, green, blue] = color.0;
        format!("#{red:02X}{green:02X}{blue:02X}")
    }
}
