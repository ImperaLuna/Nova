use crate::tokens::{
    ACCENT_FILL_OPACITY, DIM_OPACITY, FG_DIM_OPACITY, FG_SOFT_OPACITY, GLASS_HIGHLIGHT_OPACITY,
    GLASS_INNER_LINE_OPACITY, GLASS_OPACITY, GLASS_OUTLINE_OPACITY, GLASS_RIM_OPACITY, HOVER_OPACITY, METER_OPACITY,
    OUTLINE_OPACITY, RAISED_OPACITY, SELECTED_OPACITY, SELECTED_QUIET_OPACITY, SHADOW_OPACITY, SURFACE_OPACITY,
    TRACK_OPACITY,
};
use gpui::{App, Global, Hsla, Rgba, black};
use nova_theme_info::{Color, Palette, ThemeMode};
use serde::Deserialize;
use std::sync::LazyLock;

const FALLBACK_ID: &str = "carbonfox";
const FALLBACK_FILE: &str = include_str!("../../../themes/carbonfox.toml");
const CHANNEL_MAX: f32 = 255.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub mode: ThemeMode,
    pub bg: Hsla,
    pub bg_alt: Hsla,
    pub backdrop: Hsla,
    pub glass_tint: Hsla,
    pub fg: Hsla,
    pub primary: Hsla,
    pub secondary: Hsla,
    pub on_accent: Hsla,
    pub info: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
    pub swatches: [Hsla; 8],
}

impl Global for Theme {}

#[derive(Deserialize)]
struct ThemeFile {
    dark: Palette,
}

static FALLBACK: LazyLock<Theme> = LazyLock::new(|| {
    #[expect(clippy::expect_used, reason = "the file is compiled in and the_fallback_is_carbonfox_dark parses it")]
    let file: ThemeFile = toml::from_str(FALLBACK_FILE).expect("themes/carbonfox.toml is valid (the tests load it)");
    Theme::from_palette(FALLBACK_ID, ThemeMode::Dark, &file.dark)
});

impl From<&nova_theme_info::Theme> for Theme {
    fn from(theme: &nova_theme_info::Theme) -> Self {
        Self::from_palette(&theme.id, theme.mode, &theme.colors)
    }
}

impl Theme {
    pub fn of(cx: &App) -> &Theme {
        cx.try_global::<Theme>().unwrap_or(&FALLBACK)
    }

    pub fn set(theme: Theme, cx: &mut App) {
        if cx.try_global::<Theme>() == Some(&theme) {
            return;
        }
        cx.set_global(theme);
        cx.refresh_windows();
    }

    fn from_palette(id: &str, mode: ThemeMode, colors: &Palette) -> Self {
        Self {
            id: id.into(),
            mode,
            bg: opaque(&colors.bg),
            bg_alt: opaque(&colors.bg_alt),
            backdrop: opaque(&colors.backdrop),
            glass_tint: opaque(&colors.glass_tint),
            fg: opaque(&colors.fg),
            primary: opaque(&colors.primary),
            secondary: opaque(&colors.secondary),
            on_accent: opaque(&colors.on_accent),
            info: opaque(&colors.info),
            success: opaque(&colors.success),
            warning: opaque(&colors.warning),
            error: opaque(&colors.error),
            swatches: [
                &colors.swatch1,
                &colors.swatch2,
                &colors.swatch3,
                &colors.swatch4,
                &colors.swatch5,
                &colors.swatch6,
                &colors.swatch7,
                &colors.swatch8,
            ]
            .map(opaque),
        }
    }

    pub fn fg_dim(&self) -> Hsla {
        self.fg.alpha(FG_DIM_OPACITY)
    }

    pub fn fg_soft(&self) -> Hsla {
        self.fg.alpha(FG_SOFT_OPACITY)
    }

    pub fn fg_disabled(&self) -> Hsla {
        self.fg.alpha(DIM_OPACITY)
    }

    pub fn surface(&self) -> Hsla {
        self.bg.alpha(SURFACE_OPACITY)
    }

    pub fn surface_raised(&self) -> Hsla {
        self.fg.alpha(RAISED_OPACITY)
    }

    pub fn outline(&self) -> Hsla {
        self.fg.alpha(OUTLINE_OPACITY)
    }

    pub fn glass(&self) -> Hsla {
        self.glass_tint.alpha(GLASS_OPACITY)
    }

    pub fn glass_outline(&self) -> Hsla {
        black().alpha(GLASS_OUTLINE_OPACITY)
    }

    pub fn glass_inner_line(&self) -> Hsla {
        self.fg.alpha(GLASS_INNER_LINE_OPACITY)
    }

    pub fn glass_highlight(&self) -> Hsla {
        self.fg.alpha(GLASS_HIGHLIGHT_OPACITY)
    }

    pub fn glass_rim(&self) -> Hsla {
        self.fg.alpha(GLASS_RIM_OPACITY)
    }

    pub fn shadow(&self) -> Hsla {
        black().alpha(SHADOW_OPACITY)
    }

    pub fn hover(&self) -> Hsla {
        self.fg.alpha(HOVER_OPACITY)
    }

    pub fn selected(&self) -> Hsla {
        self.fg.alpha(SELECTED_OPACITY)
    }

    pub fn accent_fill(&self) -> Hsla {
        self.primary.alpha(ACCENT_FILL_OPACITY)
    }

    pub fn selected_quiet(&self) -> Hsla {
        self.fg.alpha(SELECTED_QUIET_OPACITY)
    }

    pub fn track(&self) -> Hsla {
        self.fg.alpha(TRACK_OPACITY)
    }

    pub fn meter(&self) -> Hsla {
        self.fg.alpha(METER_OPACITY)
    }

    pub fn tint(&self, target: Hsla, amount: f32) -> Hsla {
        mix(self.bg, target, amount)
    }
}

pub fn mix(from: Hsla, to: Hsla, amount: f32) -> Hsla {
    let (from, to, amount) = (Rgba::from(from), Rgba::from(to), amount.clamp(0.0, 1.0));
    let between = |from: f32, to: f32| from + (to - from) * amount;
    Rgba { r: between(from.r, to.r), g: between(from.g, to.g), b: between(from.b, to.b), a: between(from.a, to.a) }
        .into()
}

fn opaque(color: &Color) -> Hsla {
    let [red, green, blue] = color.rgb().map(|channel| f32::from(channel) / CHANNEL_MAX);
    Rgba { r: red, g: green, b: blue, a: 1.0 }.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(color: Hsla) -> [u8; 3] {
        let color = Rgba::from(color);
        [color.r, color.g, color.b].map(|channel| (channel * CHANNEL_MAX).round() as u8)
    }

    #[test]
    fn the_fallback_is_carbonfox_dark() {
        let theme = &*FALLBACK;
        assert_eq!(theme.id, "carbonfox");
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(rgb(theme.bg), [0x1D, 0x1D, 0x1D]);
        assert_eq!(rgb(theme.primary), [0xFD, 0x70, 0x14]);
        assert_eq!(rgb(theme.swatches[7]), [0x9C, 0x7C, 0x02]);
    }

    #[test]
    fn a_theme_from_the_socket_keeps_every_color() {
        let names = [
            "bg",
            "bgAlt",
            "backdrop",
            "glassTint",
            "fg",
            "primary",
            "secondary",
            "onAccent",
            "info",
            "success",
            "warning",
            "error",
            "swatch1",
            "swatch2",
            "swatch3",
            "swatch4",
            "swatch5",
            "swatch6",
            "swatch7",
            "swatch8",
        ];
        let colors: String = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let first = 1 + 3 * index;
                format!("{name} = \"#{:02X}{:02X}{:02X}\"\n", first, first + 1, first + 2)
            })
            .collect();
        let sent: nova_theme_info::Theme =
            toml::from_str(&format!("id = \"paper\"\nname = \"Paper\"\nmode = \"light\"\n[colors]\n{colors}"))
                .unwrap_or_else(|error| panic!("the theme didn't parse: {error}"));
        let theme = Theme::from(&sent);
        assert_eq!((theme.id.as_str(), theme.mode), ("paper", ThemeMode::Light));
        let colors = [
            theme.bg,
            theme.bg_alt,
            theme.backdrop,
            theme.glass_tint,
            theme.fg,
            theme.primary,
            theme.secondary,
            theme.on_accent,
            theme.info,
            theme.success,
            theme.warning,
            theme.error,
        ];
        for (index, color) in colors.into_iter().chain(theme.swatches).enumerate() {
            let first = 1 + 3 * index as u8;
            assert_eq!(rgb(color), [first, first + 1, first + 2], "color {index}");
        }
    }

    #[test]
    fn derived_fills_are_the_ink_at_a_token_s_opacity() {
        let theme = &*FALLBACK;
        assert_eq!(rgb(theme.fg_dim()), rgb(theme.fg));
        assert_eq!(theme.fg_dim().a, FG_DIM_OPACITY);
        assert_eq!(rgb(theme.glass()), rgb(theme.glass_tint));
        assert_eq!(theme.glass().a, GLASS_OPACITY);
        assert_eq!(rgb(theme.glass_outline()), [0, 0, 0]);
    }

    #[test]
    fn tint_runs_from_the_background_to_the_color() {
        let theme = &*FALLBACK;
        assert_eq!(rgb(theme.tint(theme.error, 0.0)), rgb(theme.bg));
        assert_eq!(rgb(theme.tint(theme.error, 1.0)), rgb(theme.error));
        let half = rgb(theme.tint(theme.error, 0.5));
        assert_eq!(half, [0x86, 0x38, 0x5A]);
    }

    #[test]
    fn mix_blends_alpha_too() {
        let solid = mix(FALLBACK.glass(), FALLBACK.glass_tint, 1.0);
        assert_eq!(solid.a, 1.0);
        assert!((mix(FALLBACK.glass(), FALLBACK.glass_tint, 0.5).a - (GLASS_OPACITY + 1.0) / 2.0).abs() < 1e-6);
    }
}
