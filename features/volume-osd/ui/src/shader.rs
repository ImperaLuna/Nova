//! The capsule's one shader (`liquid.wgsl`): its glass, liquid and shadow,
//! painted with the fork's `paint_shader` over the whole window, and the
//! numbers it reads.
//!
//! Its glass is its own, not the look kit's `Glass`, as it was in today's
//! shell: a sheen and a rim lit from the top left that the liquid lies
//! under, all in the one pass that also stretches with the capsule.

use crate::logic::CapsuleBox;
use crate::motion::LiquidMotion;
use crate::tokens::{
    GLOW, GLOW_DEPTH, LIQUID_MUTED_OPACITY, LIQUID_OPACITY, MENISCUS, MENISCUS_REACH, RIM_LIT_OPACITY, RIM_SHADE,
    RIM_WIDTH, SHEEN_FADE, SHEEN_OPACITY, SPECK_GLOW, SPECK_GLOW_MAX, SPECK_RADIUS, SPECK_RADIUS_RANGE, SPECKS, STREAK,
    STREAK_AT, STREAK_FADE, STREAK_WIDTH, WALL,
};
use gpui::{Bounds, Hsla, Pixels, Rgba, SHADER_PARAMS, Window, white};
use nova_look::tokens::{FG_DIM_OPACITY, SHADOW_BLUR, SHADOW_OFFSET};
use nova_look::{Theme, mix};

const SHADER: &str = include_str!("liquid.wgsl");

/// What changes the capsule's look besides its motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub capsule: CapsuleBox,
    /// The slow swell's height now (px): eased to 0 while the surface is
    /// still.
    pub swell: f32,
    /// How far the liquid has turned to the muted grey (0..1).
    pub muted: f32,
    /// There is an output, so the rim is lit (else a dashed outline
    /// replaces it).
    pub rim: bool,
}

/// Paints the capsule over `bounds` (the window).
pub fn paint(bounds: Bounds<Pixels>, look: &Look, motion: &LiquidMotion, theme: &Theme, window: &mut Window) {
    let params = params(look, motion, &Colors::of(theme, look), window.scale_factor());
    window.paint_shader(bounds, SHADER, params);
}

/// The shader's colors, from the theme.
struct Colors {
    glass: Hsla,
    sheen: Hsla,
    rim_lit: Hsla,
    liquid: Hsla,
    shadow: Hsla,
    light: Hsla,
}

impl Colors {
    fn of(theme: &Theme, look: &Look) -> Self {
        let muted = theme.fg.alpha(FG_DIM_OPACITY * LIQUID_MUTED_OPACITY);
        Self {
            glass: theme.glass(),
            sheen: theme.fg.alpha(SHEEN_OPACITY),
            // Without an output a dashed outline replaces the lit rim.
            rim_lit: theme.fg.alpha(if look.rim { RIM_LIT_OPACITY } else { 0.0 }),
            liquid: mix(theme.secondary.alpha(LIQUID_OPACITY), muted, look.muted),
            shadow: theme.shadow(),
            light: white(),
        }
    }
}

/// `liquid.wgsl`'s `quad.params`, in the window's pixels.
fn params(look: &Look, motion: &LiquidMotion, colors: &Colors, scale: f32) -> [f32; SHADER_PARAMS] {
    let CapsuleBox { x, y, w, h, radius } = look.capsule;
    let rows: [[f32; 4]; 14] = [
        [x, y, w, h],
        [radius, f32::from(WALL), motion.level.value, motion.time],
        [motion.rise, look.swell, motion.amp, motion.tilt.value],
        [MENISCUS, MENISCUS_REACH, motion.fizz, SPECKS],
        [SPECK_RADIUS, SPECK_RADIUS_RANGE, SPECK_GLOW, SPECK_GLOW_MAX],
        [GLOW, GLOW_DEPTH, STREAK, STREAK_FADE],
        [STREAK_AT, STREAK_WIDTH, RIM_WIDTH, RIM_SHADE],
        [SHEEN_FADE, f32::from(SHADOW_BLUR), f32::from(SHADOW_OFFSET), scale],
        rgba(colors.glass),
        rgba(colors.sheen),
        rgba(colors.rim_lit),
        rgba(colors.liquid),
        rgba(colors.shadow),
        rgba(colors.light),
    ];
    let mut params = [0.0; SHADER_PARAMS];
    for (slot, value) in params.iter_mut().zip(rows.into_iter().flatten()) {
        *slot = value;
    }
    params
}

/// A color premultiplied by its alpha. The shader premultiplies again
/// (`pm`), so a see-through color (the glass, the muted liquid) comes out
/// darker, as the capsule's look was tuned.
fn rgba(color: Hsla) -> [f32; 4] {
    let Rgba { r, g, b, a } = Rgba::from(color);
    [r * a, g * a, b * a, a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::capsule_box;

    #[test]
    fn every_param_has_its_slot() {
        // 14 vec4s: the shader reads them all.
        assert_eq!(SHADER_PARAMS, 14 * 4);
        let look = Look { capsule: capsule_box(1.0, 0.0, 100.0, 300.0), swell: 1.0, muted: 0.0, rim: true };
        let motion = LiquidMotion { level: nova_look::motion::SpringState::at(0.5), ..LiquidMotion::default() };
        let colors = Colors {
            glass: gpui::black(),
            sheen: gpui::black(),
            rim_lit: gpui::black(),
            liquid: gpui::black(),
            shadow: gpui::black(),
            light: white(),
        };
        let params = params(&look, &motion, &colors, 2.0);
        assert_eq!(params[6], 0.5, "the level");
        assert_eq!(params[31], 2.0, "the scale");
        assert_eq!(params[52..56], [1.0, 1.0, 1.0, 1.0], "the light");
    }

    #[test]
    fn the_shader_says_what_it_reads() {
        for function in ["fn shade(", "quad.params[13]"] {
            assert!(SHADER.contains(function), "{function}");
        }
        assert!(!SHADER.contains("quad.params[14]"));
    }
}
