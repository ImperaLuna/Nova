use crate::Theme;
use crate::tokens::{ICON_GRID, ICON_SIZE, ICON_STROKE};
use gpui::{
    App, AssetSource, Hsla, IntoElement, Pixels, RenderOnce, SharedString, Styled, Transformation, Window, radians, svg,
};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    name: &'static str,
    path: &'static str,
}

macro_rules! glyphs {
    ($( $(#[$doc:meta])* $name:ident = $path:literal; )*) => {
        impl Glyph {
            $( $(#[$doc])* pub const $name: Glyph = Glyph { name: stringify!($name), path: $path }; )*

            const ALL: &[Glyph] = &[ $( Glyph::$name, )* ];
        }
    };
}

glyphs! {
    TUNE = "M4 7h9M17 7h3M4 17h3M11 17h9M15 5a2 2 0 1 1 0 4a2 2 0 1 1 0-4zM9 15a2 2 0 1 1 0 4a2 2 0 1 1 0-4z";
    POWER = "M12 3v8M6.3 7.2a8 8 0 1 0 11.4 0";
    CUP = "M4 9h12v5a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5zM16 10h1.5a2.5 2.5 0 0 1 0 5H16M8 3.5c0 1.5 1 1.5 1 3M12 3.5c0 1.5 1 1.5 1 3";
    MIC = "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21";
    MIC_OFF = "M15 10V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5.5 11a6.5 6.5 0 0 0 10.7 5M18.5 11a6.4 6.4 0 0 1-.6 2.7M12 17.5V21M3 3l18 18";
    CLOCK = "M12 3.5a8.5 8.5 0 1 1 0 17a8.5 8.5 0 1 1 0-17zM12 7.5V12l3 2";
    RIGHT = "M9 6l6 6-6 6";
    LEFT = "M15 6l-6 6 6 6";
    CHECK = "M5 12.5l4.5 4.5L19 7.5";
    SPEAKER_BODY = "M4 9h4l5-4v14l-5-4H4z";
    WAVE = "M16.5 8.5a5 5 0 0 1 0 7";
    WAVES = "M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12";
    SPEAKER = "M4 9h4l5-4v14l-5-4H4zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12";
    MUTED = "M16.5 9.5l5 5M21.5 9.5l-5 5";
    STRIKE = "M3 3l18 18";
    HEADPHONES_BAND = "M4 15v-3a8 8 0 0 1 16 0v3";
    HEADPHONES = "M4 15v-3a8 8 0 0 1 16 0v3M5 14h2.5v6H5a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1zM16.5 14H19a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-2.5z";
    HEADPHONES_CUPS = "M4 15h3.5v6H5.5A1.5 1.5 0 0 1 4 19.5zM20 15h-3.5v6h2a1.5 1.5 0 0 0 1.5-1.5z";
    MONITOR = "M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8 20h8M12 16v4";
    MONITORS = "M3.5 4h9A1.5 1.5 0 0 1 14 5.5V9h-3.5A1.5 1.5 0 0 0 9 10.5V13H3.5A1.5 1.5 0 0 1 2 11.5v-6A1.5 1.5 0 0 1 3.5 4zM10.5 9h10a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5h-10A1.5 1.5 0 0 1 9 16.5v-6A1.5 1.5 0 0 1 10.5 9zM13 21h5";
    BLUETOOTH = "M7 7l10 10-5 4V3l5 4L7 17";
    CLOSE = "M6 6l12 12M18 6L6 18";
    BELL = "M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0";
    BELL_OFF = "M6 16V11a6 6 0 0 1 9.5-4.9M18 11v5l1.5 2H8M10 20.5a2 2 0 0 0 4 0M3 3l18 18";
    GEAR = "M12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";
    DOTS = "M6 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM12 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM18 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4z";
    SEARCH = "M10.5 4a6.5 6.5 0 1 1 0 13a6.5 6.5 0 0 1 0-13zM15.3 15.3L20 20";
    TILDE = "M5 13.5C7 10.5 9.5 10.5 12 12S17 13.5 19 10.5";
    PIN = "M9 4h6M10 4v5.5L7 13v1.5h10V13l-3-3.5V4M12 14.5V20";
    STAR = "M12 3c.6 4.6 2.4 6.4 7 7c-4.6.6-6.4 2.4-7 7c-.6-4.6-2.4-6.4-7-7c4.6-.6 6.4-2.4 7-7z";
    REGION = "M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3";
    OPEN = "M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4";
    FOLDER = "M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z";
    TRASH = "M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3";
    CAMERA = "M4 8h3l2-3h6l2 3h3v11H4zM8.5 13a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0-7 0";
    VIDEO = "M4 7a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2zM16 10l4-2.5v9L16 14";
    VIDEO_OFF = "M4 7a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2zM16 10l4-2.5v9L16 14M3 3l18 18";
    FILE = "M7 3h7l4 4v12a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM14 3v4h4";
    DOCUMENT = "M7 3h7l4 4v12a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM14 3v4h4M9 12h6M9 16h6";
    PAUSE = "M9 6v12M15 6v12";
    PAUSE_SOLID = "M7.7 5h1.6a1.2 1.2 0 0 1 1.2 1.2v11.6a1.2 1.2 0 0 1-1.2 1.2H7.7a1.2 1.2 0 0 1-1.2-1.2V6.2A1.2 1.2 0 0 1 7.7 5zM14.7 5h1.6a1.2 1.2 0 0 1 1.2 1.2v11.6a1.2 1.2 0 0 1-1.2 1.2h-1.6a1.2 1.2 0 0 1-1.2-1.2V6.2a1.2 1.2 0 0 1 1.2-1.2z";
    PLAY_SOLID = "M8 5l11 7-11 7z";
    SKIP_NEXT = "M6 5l9 7-9 7zM18 5v14";
    SKIP_NEXT_FILL = "M6 5l9 7-9 7z";
    SKIP_PREVIOUS = "M18 5l-9 7 9 7zM6 5v14";
    SKIP_PREVIOUS_FILL = "M18 5l-9 7 9 7z";
    MUSIC = "M9 18V5l11-2v13M6.5 15.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5zM17.5 13.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5z";
    GO_TO = "M7 17L17 7M9 7h8v8";
    PLAY = "M8 5.5v13l10-6.5z";
    STOP = "M7 7h10v10H7z";
    LOCK = "M7.5 11h9a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-2.5 2.5h-9a2.5 2.5 0 0 1-2.5-2.5v-5a2.5 2.5 0 0 1 2.5-2.5zM8 11V8a4 4 0 0 1 8 0v3";
    MOON = "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z";
    LOG_OUT = "M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M10 8l-4 4 4 4M6 12h10";
    REBOOT = "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4";
    MOUSE = "M12 3a6 6 0 0 1 6 6v6a6 6 0 0 1-12 0V9a6 6 0 0 1 6-6zM12 7v4";
    KEYBOARD = "M4.5 6h15a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2zM6 10h.01M9.5 10h.01M13 10h.01M16.5 10h.01M7 14h10";
    GAMEPAD = "M7 8h10a4 4 0 0 1 4 4.5l-.6 3.6a2 2 0 0 1-3.5 1L15 15H9l-1.9 2.1a2 2 0 0 1-3.5-1L3 12.5A4 4 0 0 1 7 8zM7.5 11v3M6 12.5h3M15.5 12h.01M17.5 13.5h.01";
    DEVICE = "M8 3h8a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM11 18h2";
    DONGLE = "M5 12a7 7 0 0 1 14 0M8 12a4 4 0 0 1 8 0M12 13h.01";
    PLUG = "M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0zM12 16v5";
    BOLT = "M13 2L4 14h7l-1 8 9-12h-7z";
    WIFI_ARC_OUTER = "M2 8.5a15 15 0 0 1 20 0";
    WIFI_ARC_MIDDLE = "M5 12a10.5 10.5 0 0 1 14 0";
    WIFI_ARC_INNER = "M8.5 15.5a5.5 5.5 0 0 1 7 0";
    WIFI = "M2 8.5a15 15 0 0 1 20 0M5 12a10.5 10.5 0 0 1 14 0M8.5 15.5a5.5 5.5 0 0 1 7 0";
    WIFI_DOT = "M12 17.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4z";
    WIFI_OFF = "M2 8.5a15 15 0 0 1 4.6-2.9M10.5 4.6A15 15 0 0 1 22 8.5M5 12a10.5 10.5 0 0 1 3.2-2M15 9.6a10.5 10.5 0 0 1 4 2.4M8.5 15.5a5.5 5.5 0 0 1 7 0M3 3l18 18";
    PORT = "M7.5 4h9A1.5 1.5 0 0 1 18 5.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 12.5v-7A1.5 1.5 0 0 1 7.5 4zM9 14v3h6v-3M12 17v3";
    WIRED = "M7.5 4h9A1.5 1.5 0 0 1 18 5.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 12.5v-7A1.5 1.5 0 0 1 7.5 4zM9 14v3h6v-3M12 17v3M9 7v2M12 7v2M15 7v2";
    WIRED_OFF = "M7.5 4h9A1.5 1.5 0 0 1 18 5.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 12.5v-7A1.5 1.5 0 0 1 7.5 4zM9 14v3h6v-3M12 17v3M3 3l18 18";
    EYE = "M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12zM12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6z";
    EYE_OFF = "M10 5.7A9.6 9.6 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-2.6 3.4M6.6 6.6C3.9 8.3 2.5 12 2.5 12s3.5 6.5 9.5 6.5a9.3 9.3 0 0 0 5.4-1.7M9.9 9.9a3 3 0 0 0 4.2 4.2M3 3l18 18";
    PICTURE = "M5.5 5h13a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM6 17l4.5-5 3 3.5 2-2 3 3.5M15.5 7.5a1.5 1.5 0 1 1 0 3a1.5 1.5 0 1 1 0-3z";
}

const ASSET_ROOT: &str = "nova-icon";
const ASSET_EXTENSION: &str = ".svg";
const NO_FILL: &str = "-";
const STROKE_STEPS_PER_UNIT: f32 = 100.0;

#[derive(Debug, Clone, Copy, IntoElement)]
pub struct Icon {
    glyph: Glyph,
    fill: Option<Glyph>,
    size: Pixels,
    stroke: f32,
    color: Option<Hsla>,
    turned: f32,
}

impl Icon {
    pub fn new(glyph: Glyph) -> Self {
        Self { glyph, fill: None, size: ICON_SIZE, stroke: ICON_STROKE, color: None, turned: 0.0 }
    }

    pub fn filled(mut self, glyph: Glyph) -> Self {
        self.fill = Some(glyph);
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn stroke(mut self, width: f32) -> Self {
        self.stroke = width;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn turned(mut self, degrees: f32) -> Self {
        self.turned = degrees;
        self
    }

    fn asset_path(&self) -> String {
        let fill = self.fill.map_or(NO_FILL, |glyph| glyph.name);
        let stroke = (self.stroke * STROKE_STEPS_PER_UNIT).round() as u32;
        format!("{ASSET_ROOT}/{}/{fill}/{stroke}{ASSET_EXTENSION}", self.glyph.name)
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(Theme::of(cx).fg);
        svg()
            .path(self.asset_path())
            .size(self.size)
            .flex_none()
            .text_color(color)
            .with_transformation(Transformation::rotate(radians(self.turned.to_radians())))
    }
}

pub struct IconAssets;

impl AssetSource for IconAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(document(path).map(|svg| Cow::Owned(svg.into_bytes())))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

fn glyph_named(name: &str) -> Option<Glyph> {
    Glyph::ALL.iter().copied().find(|glyph| glyph.name == name)
}

fn document(path: &str) -> Option<String> {
    let mut parts = path.strip_prefix(ASSET_ROOT)?.strip_prefix('/')?.strip_suffix(ASSET_EXTENSION)?.split('/');
    let (stroked, fill, stroke) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let stroked = glyph_named(stroked)?;
    let width = stroke.parse::<u32>().ok()? as f32 / STROKE_STEPS_PER_UNIT;
    let solid = match fill {
        NO_FILL => String::new(),
        name => format!(r#"<path fill="black" d="{}"/>"#, glyph_named(name)?.path),
    };
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {ICON_GRID} {ICON_GRID}">{solid}<path fill="none" stroke="black" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round" d="{}"/></svg>"#,
        stroked.path
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_s_path_gives_back_its_document() {
        let path = Icon::new(Glyph::SEARCH).asset_path();
        assert_eq!(path, "nova-icon/SEARCH/-/180.svg");
        let svg = document(&path).unwrap_or_else(|| panic!("no document for {path}"));
        assert!(svg.contains(r#"viewBox="0 0 24 24""#), "{svg}");
        assert!(svg.contains(r#"stroke-width="1.8""#), "{svg}");
        assert!(svg.contains(Glyph::SEARCH.path), "{svg}");
        assert!(!svg.contains(r#"fill="black""#), "{svg}");
    }

    #[test]
    fn a_filled_icon_draws_the_solid_glyph_under_the_strokes() {
        let path = Icon::new(Glyph::WIFI).filled(Glyph::WIFI_DOT).stroke(1.25).asset_path();
        assert_eq!(path, "nova-icon/WIFI/WIFI_DOT/125.svg");
        let svg = document(&path).unwrap_or_else(|| panic!("no document for {path}"));
        let (solid, strokes) = (svg.find(Glyph::WIFI_DOT.path), svg.find(r#"fill="none""#));
        assert!(solid.is_some() && solid < strokes, "{svg}");
    }

    #[test]
    fn other_paths_are_not_icons() {
        for path in [
            "nova-icon/NOPE/-/180.svg",
            "nova-icon/SEARCH/NOPE/180.svg",
            "nova-icon/SEARCH/-/wide.svg",
            "nova-icon/SEARCH/-/180.svg/more.svg",
            "nova-icon/SEARCH/-/180.png",
            "nova-iconSEARCH/-/180.svg",
            "images/SEARCH/-/180.svg",
            "",
        ] {
            assert_eq!(document(path), None, "{path}");
        }
    }
}
