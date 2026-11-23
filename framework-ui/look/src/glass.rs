//! Nova's smoked glass, the surface pills and panels are made of.
//!
//! The compositor blurs what is behind the glass (its window's blur region,
//! `crate::shape`); this draws what lies on that blur, back to front:
//!
//! - a soft shadow around it, cut out where the glass is, since the glass
//!   is see-through and a shadow under it would only darken it. GPUI's own
//!   shadows can't be cut out, so one small shader draws it
//!   (`glass_shadow.wgsl`, through the fork's `paint_shader`);
//! - the tint: the theme's warm near-black, thin enough that the colors
//!   behind still show;
//! - its edge: a dark outline, a faint light line just inside it, and a
//!   highlight along the top that fades out before the corners.
//!
//! [`Glass::build`] gives a `Div` with all of that already in it. The
//! caller sizes and places it and adds its content, which lies on top and
//! is laid out in the glass's full box (the edge is drawn over it, it takes
//! no room). The shadow reaches `SHADOW_ROOM` past the glass on every
//! side: the window must be that much bigger, and nothing may clip there.
//! That includes the glass itself: `overflow_hidden` on it cuts its own
//! shadow off, so content that must be clipped to the glass (a panel while
//! it grows) goes in a child that hides its overflow.
//!
//! GPUI clips a rounded element's children to its box, not its corners. So
//! content that touches the glass's edge rounds its own corners to match,
//! with [`crate::LookStyled::fit_corners`].
//!
//! The fork draws at most 32 shader rectangles per window and frame and
//! skips the rest, so at most that many glasses with a shadow fit a window.

use crate::Theme;
use crate::tokens::{
    GLASS_EDGE_WIDTH, GLASS_HIGHLIGHT_FADE, GLASS_HIGHLIGHT_INSET, GLASS_OPACITY, SHADOW_BLUR, SHADOW_OFFSET,
    SHADOW_ROOM,
};
use gpui::{
    App, Bounds, Div, Hsla, ParentElement, Pixels, Rgba, SHADER_PARAMS, Styled, canvas, div, linear_color_stop,
    linear_gradient, px, relative,
};

const SHADOW_SHADER: &str = include_str!("glass_shadow.wgsl");
/// A blur of radius r, as CSS counts it, is a Gaussian of sigma r/2.
const SIGMA_PER_BLUR_RADIUS: f32 = 0.5;
/// A gradient that runs left to right, in CSS degrees.
const TO_THE_RIGHT: f32 = 90.0;

/// A piece of glass with corners of one radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glass {
    radius: Pixels,
    tint_opacity: f32,
    shadow: f32,
    rim_cut: Pixels,
}

impl Glass {
    pub fn new(radius: Pixels) -> Self {
        Self { radius, tint_opacity: GLASS_OPACITY, shadow: 1.0, rim_cut: px(0.0) }
    }

    /// How much of the glass is its tint (0..1), for glass that must be
    /// thicker or thinner than the look's own.
    pub fn tint_opacity(mut self, opacity: f32) -> Self {
        self.tint_opacity = opacity;
        self
    }

    /// The shadow's strength (0 none, 1 full). Glass that fades with
    /// `.opacity()` passes the same value here: the shader that draws the
    /// shadow doesn't see an element's opacity.
    pub fn shadow(mut self, strength: f32) -> Self {
        self.shadow = strength.clamp(0.0, 1.0);
        self
    }

    /// Stops its edge this far short of its right side, for glass whose
    /// outline runs on into something drawn beside it (the power menu's
    /// tab into its fillets).
    pub fn rim_cut(mut self, cut: Pixels) -> Self {
        self.rim_cut = cut.max(px(0.0));
        self
    }

    /// The glass as an element to size, place and fill.
    pub fn build(self, cx: &App) -> Div {
        let theme = Theme::of(cx);
        let Self { radius, tint_opacity, shadow, rim_cut } = self;
        let inner_radius = (radius - GLASS_EDGE_WIDTH).max(px(0.0));
        let highlight = theme.glass_highlight();
        let fade = |from: Hsla, to: Hsla| {
            div().h_full().w(relative(GLASS_HIGHLIGHT_FADE)).bg(linear_gradient(
                TO_THE_RIGHT,
                linear_color_stop(from, 0.0),
                linear_color_stop(to, 1.0),
            ))
        };
        let glass = div().rounded(radius).bg(theme.glass_tint.alpha(tint_opacity));
        let glass =
            if shadow > 0.0 { glass.child(shadow_under(radius, theme.shadow().opacity(shadow))) } else { glass };
        // The outline, then the light line just inside it, cut short of the
        // right side by `rim_cut`.
        let edge = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .right(-rim_cut)
            .child(
                div().absolute().inset_0().rounded(radius).border(GLASS_EDGE_WIDTH).border_color(theme.glass_outline()),
            )
            .child(
                div()
                    .absolute()
                    .inset(GLASS_EDGE_WIDTH)
                    .rounded(inner_radius)
                    .border(GLASS_EDGE_WIDTH)
                    .border_color(theme.glass_inner_line()),
            );
        let edge = if rim_cut > px(0.0) {
            div().absolute().top_0().bottom_0().left_0().right(rim_cut).overflow_hidden().child(edge)
        } else {
            edge
        };
        glass
            .child(edge)
            // The highlight along the top, between the corners' curves.
            .child(
                div()
                    .absolute()
                    .top(GLASS_EDGE_WIDTH)
                    .left(radius * GLASS_HIGHLIGHT_INSET)
                    .right(radius * GLASS_HIGHLIGHT_INSET + rim_cut)
                    .h(GLASS_EDGE_WIDTH)
                    .flex()
                    .child(fade(highlight.alpha(0.0), highlight))
                    .child(div().h_full().flex_1().bg(highlight))
                    .child(fade(highlight, highlight.alpha(0.0))),
            )
    }
}

/// The shadow of a glass of `radius`, as a child that covers the glass and
/// the shadow's room around it.
fn shadow_under(radius: Pixels, color: Hsla) -> impl gpui::IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let params = shadow_params(bounds, radius, color, window.scale_factor());
            window.paint_shader(bounds, SHADOW_SHADER, params);
        },
    )
    .absolute()
    .inset(-SHADOW_ROOM)
}

/// What `glass_shadow.wgsl` reads, for a shadow painted over `bounds` (the
/// glass and its room) on a window that draws `scale` device pixels per
/// pixel.
fn shadow_params(bounds: Bounds<Pixels>, radius: Pixels, color: Hsla, scale: f32) -> [f32; SHADER_PARAMS] {
    let device = |length: Pixels| f32::from(length) * scale;
    let half_width = device(bounds.size.width / 2.0 - SHADOW_ROOM).max(0.0);
    let half_height = device(bounds.size.height / 2.0 - SHADOW_ROOM).max(0.0);
    let color = Rgba::from(color);
    let rows = [
        [half_width, half_height, device(radius).min(half_width).min(half_height), 0.0],
        [device(SHADOW_OFFSET), device(SHADOW_BLUR) * SIGMA_PER_BLUR_RADIUS, 0.0, 0.0],
        [color.r, color.g, color.b, color.a],
    ];
    let mut params = [0.0; SHADER_PARAMS];
    for (slot, value) in params.iter_mut().zip(rows.into_iter().flatten()) {
        *slot = value;
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{black, point, size};

    #[test]
    fn the_shader_gets_the_glass_in_device_pixels() {
        // A 300 x 200 glass with its room, on a screen at twice the scale.
        let room = f32::from(SHADOW_ROOM);
        let bounds =
            Bounds { origin: point(px(0.0), px(0.0)), size: size(px(300.0 + 2.0 * room), px(200.0 + 2.0 * room)) };
        let params = shadow_params(bounds, px(24.0), black().alpha(0.35), 2.0);
        assert_eq!(params[..4], [300.0, 200.0, 48.0, 0.0]);
        assert_eq!(params[4..8], [20.0, 30.0, 0.0, 0.0]);
        assert_eq!(params[8..12], [0.0, 0.0, 0.0, 0.35]);
        assert!(params[12..].iter().all(|&value| value == 0.0));
    }

    #[test]
    fn a_pill_s_radius_never_passes_half_its_height() {
        let room = f32::from(SHADOW_ROOM);
        let bounds =
            Bounds { origin: point(px(0.0), px(0.0)), size: size(px(200.0 + 2.0 * room), px(36.0 + 2.0 * room)) };
        let params = shadow_params(bounds, px(500.0), black(), 1.0);
        assert_eq!(params[..3], [100.0, 18.0, 18.0]);
    }
}
