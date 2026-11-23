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
const SIGMA_PER_BLUR_RADIUS: f32 = 0.5;
const TO_THE_RIGHT: f32 = 90.0;

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

    pub fn tint_opacity(mut self, opacity: f32) -> Self {
        self.tint_opacity = opacity;
        self
    }

    pub fn shadow(mut self, strength: f32) -> Self {
        self.shadow = strength.clamp(0.0, 1.0);
        self
    }

    pub fn rim_cut(mut self, cut: Pixels) -> Self {
        self.rim_cut = cut.max(px(0.0));
        self
    }

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
        glass.child(edge).child(
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
