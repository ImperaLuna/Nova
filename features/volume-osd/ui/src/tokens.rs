//! The volume capsule's own sizes, timings and springs. Until the
//! Quickshell shell is removed the same values live in
//! `ui/theme/Tokens.qml`, under the names the test below lists: change a
//! value in both.

use gpui::{Pixels, px};
use nova_look::motion::Spring;
use std::time::Duration;

// The capsule: this big, this far from the right edge, with glass this
// thick between its rim and the liquid. The speaker and number sit
// LABEL_PAD above its bottom and fade in over the last LABEL_FADE_IN of
// its way out.
pub const WIDTH: Pixels = px(48.0);
pub const HEIGHT: Pixels = px(260.0);
pub const EDGE_GAP: Pixels = px(12.0);
pub const WALL: Pixels = px(4.0);
pub const LABEL_PAD: Pixels = px(13.0);
pub const LABEL_FADE_IN: f32 = 0.4;
/// While it slides it stretches (thinner and taller) by up to STRETCH, all
/// of it from STRETCH_SPEED on (its way out per second), and it overshoots
/// its full height by up to OVERSHOOT as it lands.
pub const STRETCH: f32 = 0.10;
pub const STRETCH_SPEED: f32 = 6.0;
pub const OVERSHOOT: f32 = 0.08;
/// How long it stays out after the last change.
pub const HOLD: Duration = Duration::from_millis(1500);
/// How far a press moves before it's a drag, not a click: Qt's
/// `startDragDistance`, which today's shell takes from the platform.
pub const DRAG_DISTANCE: Pixels = px(10.0);
/// Its window while nothing is out: as small as the compositor takes.
pub const RESTING: Pixels = px(1.0);
/// Points along each corner of the dashed outline with no output.
pub const CORNER_POINTS: usize = 12;
/// The longest time one frame of its motion covers, so a frame after a
/// pause doesn't throw the springs; and how far out it still counts as
/// out on its way in.
pub const FRAME_MAX: Duration = Duration::from_millis(33);
pub const GONE: f32 = 0.002;
/// The longest it waits for its window to grow before it comes out in the
/// one it has: a compositor answers within a frame or two.
pub const GROW_WAIT: Duration = Duration::from_millis(100);

/// The liquid, and the grey still liquid of a muted output (the ink at
/// `FG_DIM_OPACITY` times this).
pub const LIQUID_OPACITY: f32 = 0.95;
pub const LIQUID_MUTED_OPACITY: f32 = 0.7;
// Its surface (px): a slow swell, the waves a change stirs up (at most),
// the swell kept while it sits still, how far the surface creeps up the
// glass and how far from the glass that fades out.
pub const SWELL: f32 = 1.8;
pub const SLOSH_MAX: f32 = 6.0;
pub const IDLE_WAVE: f32 = 1.2;
pub const MENISCUS: f32 = 1.6;
pub const MENISCUS_REACH: f32 = 5.0;
/// How fast stirred waves settle (s), and how strongly the level's speed
/// stirs them.
pub const SLOSH_DECAY: f32 = 0.45;
pub const SLOSH_GAIN: f32 = 32.0;
/// Specks drifting up through it: most at once, and the share always
/// rising while it's calm.
pub const SPECKS: f32 = 14.0;
pub const FIZZ: f32 = 0.3;
// A speck's radius (px): the smallest, and how much bigger one can be; how
// bright they glow, and the brightest a cluster gets (0..1); how much
// quicker they rise at full fizz.
pub const SPECK_RADIUS: f32 = 0.8;
pub const SPECK_RADIUS_RANGE: f32 = 1.5;
pub const SPECK_GLOW: f32 = 0.7;
pub const SPECK_GLOW_MAX: f32 = 0.85;
pub const FIZZ_PACE: f32 = 2.0;
// Light inside it: just under the surface (how strong, how deep in px),
// and a streak down the left side (how strong, how far down it grows in
// px, where it runs across the liquid and how wide, shares of its width).
pub const GLOW: f32 = 0.28;
pub const GLOW_DEPTH: f32 = 6.0;
pub const STREAK: f32 = 0.16;
pub const STREAK_FADE: f32 = 18.0;
pub const STREAK_AT: f32 = 0.18;
pub const STREAK_WIDTH: f32 = 0.07;
/// How much the capsule's sideways acceleration tilts the surface, and the
/// steepest tilt (px per px).
pub const TILT_GAIN: f32 = 0.005;
pub const TILT_MAX: f32 = 0.5;

// The capsule's glass: a sheen across the top that has faded SHEEN_FADE of
// the way down, and a rim lit from the top left, RIM_SHADE as bright on
// its unlit side. The sheen and lit rim are the ink at these opacities;
// with no output a dashed outline in RIM_GLOW replaces the rim.
pub const SHEEN_OPACITY: f32 = 0.12;
pub const SHEEN_FADE: f32 = 0.45;
pub const RIM_WIDTH: f32 = 1.2;
pub const RIM_SHADE: f32 = 0.35;
pub const RIM_LIT_OPACITY: f32 = 0.60;
pub const RIM_GLOW_OPACITY: f32 = 0.36;

/// The springs: the capsule sliding, the level following the volume, and
/// the surface swinging back after a tilt.
pub const SLIDE: Spring = Spring { stiffness: 170.0, damping: 0.62 };
pub const LEVEL: Spring = Spring { stiffness: 70.0, damping: 0.85 };
pub const TILT: Spring = Spring { stiffness: 90.0, damping: 0.18 };

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOKENS_QML: &str = include_str!("../../../../ui/theme/Tokens.qml");

    fn qml_properties() -> HashMap<&'static str, &'static str> {
        TOKENS_QML
            .lines()
            .filter_map(|line| {
                let mut words = line.trim().strip_prefix("readonly property ")?.splitn(2, ' ');
                let _kind = words.next()?;
                let (name, value) = words.next()?.split_once(':')?;
                Some((name.trim(), value.trim()))
            })
            .collect()
    }

    /// A number, or the opacity of `alpha(color, opacity)`.
    fn number(value: &str) -> Option<f64> {
        let opacity = value.strip_suffix(')').and_then(|call| call.rsplit(',').next());
        opacity.unwrap_or(value).trim().parse().ok()
    }

    #[test]
    fn every_token_has_tokens_qml_s_value() {
        let pixels = |value: Pixels| f64::from(f32::from(value));
        let ratio = f64::from;
        let ours: &[(&str, f64)] = &[
            ("capsuleWidth", pixels(WIDTH)),
            ("capsuleHeight", pixels(HEIGHT)),
            ("capsuleEdgeGap", pixels(EDGE_GAP)),
            ("capsuleWall", pixels(WALL)),
            ("capsuleLabelPad", pixels(LABEL_PAD)),
            ("capsuleLabelFadeIn", ratio(LABEL_FADE_IN)),
            ("capsuleStretch", ratio(STRETCH)),
            ("capsuleStretchSpeed", ratio(STRETCH_SPEED)),
            ("capsuleOvershoot", ratio(OVERSHOOT)),
            ("durationCapsuleHold", HOLD.as_millis() as f64),
            ("liquidOpacity", ratio(LIQUID_OPACITY)),
            ("liquidMutedOpacity", ratio(LIQUID_MUTED_OPACITY)),
            ("liquidSwell", ratio(SWELL)),
            ("liquidSloshMax", ratio(SLOSH_MAX)),
            ("liquidIdleWave", ratio(IDLE_WAVE)),
            ("liquidMeniscus", ratio(MENISCUS)),
            ("liquidMeniscusReach", ratio(MENISCUS_REACH)),
            ("liquidSloshDecay", ratio(SLOSH_DECAY)),
            ("liquidSloshGain", ratio(SLOSH_GAIN)),
            ("liquidSpecks", ratio(SPECKS)),
            ("liquidFizz", ratio(FIZZ)),
            ("liquidSpeckRadius", ratio(SPECK_RADIUS)),
            ("liquidSpeckRadiusRange", ratio(SPECK_RADIUS_RANGE)),
            ("liquidSpeckGlow", ratio(SPECK_GLOW)),
            ("liquidSpeckGlowMax", ratio(SPECK_GLOW_MAX)),
            ("liquidFizzPace", ratio(FIZZ_PACE)),
            ("liquidGlow", ratio(GLOW)),
            ("liquidGlowDepth", ratio(GLOW_DEPTH)),
            ("liquidStreak", ratio(STREAK)),
            ("liquidStreakFade", ratio(STREAK_FADE)),
            ("liquidStreakAt", ratio(STREAK_AT)),
            ("liquidStreakWidth", ratio(STREAK_WIDTH)),
            ("liquidTiltGain", ratio(TILT_GAIN)),
            ("liquidTiltMax", ratio(TILT_MAX)),
            ("glassSheen", ratio(SHEEN_OPACITY)),
            ("glassVeilAt", ratio(SHEEN_FADE)),
            ("glassRimWidth", ratio(RIM_WIDTH)),
            ("glassRimShade", ratio(RIM_SHADE)),
            ("glassRimLit", ratio(RIM_LIT_OPACITY)),
            ("glassRimGlow", ratio(RIM_GLOW_OPACITY)),
            ("springCapsule", ratio(SLIDE.stiffness)),
            ("springCapsuleDamping", ratio(SLIDE.damping)),
            ("springLevel", ratio(LEVEL.stiffness)),
            ("springLevelDamping", ratio(LEVEL.damping)),
            ("springTilt", ratio(TILT.stiffness)),
            ("springTiltDamping", ratio(TILT.damping)),
            ("volumeStep", ratio(nova_sound_api::VOLUME_STEP)),
        ];
        let qml = qml_properties();
        for (name, value) in ours {
            let text = qml.get(name).unwrap_or_else(|| panic!("Tokens.qml has no {name}"));
            let theirs = number(text).unwrap_or_else(|| panic!("Tokens.qml's {name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
