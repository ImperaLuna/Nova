//! One card of the deck, turned as `tilt.rs` works out and drawn by
//! `card.wgsl` through the fork's `paint_shader_image` (GPUI has no 3D
//! transforms; a shader that reads the picture does the turn). The shader
//! draws the picture, its rounding, its dimming and its ring; the chips
//! are text, which a shader can't turn, so they stay flat on the turned
//! card's corners. A flat chip is wider than today's turned one: where a
//! nearer card hides most of today's chip, `draw.rs` leaves the flat one
//! out ([`ChipPlaces`]), so no sliver of it shows.

use crate::tilt::Tilt;
use crate::tokens::{LEGEND_DOT, LOADING, RING};
use gpui::{
    App, Bounds, Div, Hsla, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, RenderImage,
    Rgba, SHADER_PARAMS, SharedString, Styled, TextRun, Window, canvas, div, font, px,
};
use nova_look::tokens::{
    CHIP_HEIGHT, CHIP_PAD, FONT_MONO, FONT_SANS, FONT_SIZE_SMALL, ICON_GAP, PAD, RADIUS_CARD, WEIGHT_LABEL,
};
use nova_look::{Family, Theme};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// The card's shader with a picture to read, and with the quiet color of a
/// card still loading instead.
const WITH_PICTURE: &str =
    concat!(include_str!("card.wgsl"), "\nfn picture(uv: vec2<f32>) -> vec4<f32> { return image(uv); }\n");
const LOADING_CARD: &str =
    concat!(include_str!("card.wgsl"), "\nfn picture(uv: vec2<f32>) -> vec4<f32> { return quad.params[5]; }\n");

/// Room round the turned card for its soft edge.
const EDGE: f32 = 1.0;

/// How a card looks.
pub struct CardLook {
    /// Its image once decoded.
    pub picture: Option<Arc<RenderImage>>,
    pub picked: bool,
    pub now: bool,
    /// The other screens picking it.
    pub marks: Vec<String>,
    /// How much darker it is than the middle card (0 to 1).
    pub dim: f32,
    /// How much of it shows (0 to 1), the deck's own fade included: the
    /// shader isn't faded by the elements round it.
    pub opacity: f32,
    /// The card's size beside the middle card's (1 there): Qt scales its
    /// rounding, its ring and its chips' place with it.
    pub sized: f32,
}

/// Where today's shell has a card's chips, as stretches of the card along
/// their middle line (two of the card's points each): "Now" from the
/// right, the other screens' names from the left. Today's chips turn with
/// the card, so these say what of them a nearer card leaves in view.
pub struct ChipPlaces {
    pub now: [(f32, f32); 2],
    pub marks: [(f32, f32); 2],
}

impl ChipPlaces {
    pub fn of(tilt: &Tilt, sized: f32, marks: &[String], window: &Window) -> Self {
        let chip = |text: &str, family: Family, extra: Pixels| {
            f32::from(CHIP_PAD * 2.0 + extra + text_width(text, family, window))
        };
        let now_width = chip("Now", FONT_SANS, LEGEND_DOT + ICON_GAP);
        let gaps = f32::from(ICON_GAP) * marks.len().saturating_sub(1) as f32;
        let marks_width = marks.iter().map(|mark| chip(mark, FONT_MONO, px(0.0))).sum::<f32>() + gaps;
        let pad = f32::from(PAD) * sized;
        let y = -tilt.half_height + pad + f32::from(CHIP_HEIGHT) * sized / 2.0;
        let (right, left) = (tilt.half_width - pad, -tilt.half_width + pad);
        Self { now: [(right - now_width * sized, y), (right, y)], marks: [(left, y), (left + marks_width * sized, y)] }
    }
}

/// How wide `text` is in a chip.
fn text_width(text: &str, family: Family, window: &Window) -> Pixels {
    let mut face = font(family);
    face.weight = WEIGHT_LABEL;
    let run = TextRun {
        len: text.len(),
        font: face,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(SharedString::from(text.to_owned()), FONT_SIZE_SMALL, &[run], None).width
}

/// What the shader is told about the card, in pixels (`card.wgsl` lists
/// them); [`params`] turns them into the screen's.
struct Painted {
    tilt: Tilt,
    radius: f32,
    ring: f32,
    opacity: f32,
    dim: f32,
    dim_color: Hsla,
    ring_color: Hsla,
    loading: Hsla,
}

/// One image in the deck, turned by `tilt` with its middle at `middle`: the
/// picture cut to the card's rounding (a quiet card until it loads),
/// dimmed, ringed while picked, marked "Now" while it's the screen's
/// wallpaper, and with the names of the other screens picking it. The
/// element is the box the turned card fills; a click on the card itself is
/// `clicked`'s, one in the box's corners beside its slanted edges goes on
/// to what's under.
pub fn draw_card(
    look: CardLook,
    tilt: Tilt,
    middle: Point<Pixels>,
    clicked: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let theme = Theme::of(cx);
    let ring = if look.picked { f32::from(RING) * look.sized } else { 0.0 };
    let [left, top, right, bottom] = tilt.reach(ring + EDGE);
    // The box, on whole pixels.
    let (left, top) = ((f32::from(middle.x) + left).floor(), (f32::from(middle.y) + top).floor());
    let (right, bottom) = ((f32::from(middle.x) + right).ceil(), (f32::from(middle.y) + bottom).ceil());
    let in_box = (f32::from(middle.x) - left, f32::from(middle.y) - top);
    let painted = Painted {
        tilt,
        radius: f32::from(RADIUS_CARD) * look.sized,
        ring,
        opacity: look.opacity,
        dim: look.dim,
        dim_color: theme.backdrop,
        ring_color: theme.fg,
        loading: theme.fg.opacity(LOADING),
    };
    let picture = look.picture;
    // Where the box is in the window, for the click.
    let corner = Rc::new(Cell::new(Point::default()));
    let painted_at = corner.clone();
    let shader = canvas(
        move |bounds, _, _| painted_at.set(bounds.origin),
        move |bounds, (), window, _| {
            let params = params(&painted, bounds, in_box, window.scale_factor());
            let drawn = picture
                .as_ref()
                .is_some_and(|picture| window.paint_shader_image(bounds, WITH_PICTURE, params, picture).is_ok());
            if !drawn {
                window.paint_shader(bounds, LOADING_CARD, params);
            }
        },
    )
    .absolute()
    .size_full();

    // The chips, flat, their corner on the turned card's point they hang
    // from in today's shell.
    let pad = f32::from(PAD) * look.sized;
    let hang = |x: f32| {
        let (x, y) = tilt.onto_screen(x, -tilt.half_height + pad);
        (px(in_box.0 + x), px(in_box.1 + y))
    };
    let chip = || div().h(CHIP_HEIGHT).px(CHIP_PAD).rounded_full().bg(theme.surface()).flex().items_center();
    let now = look.now.then(|| {
        let (x, y) = hang(tilt.half_width - pad);
        chip()
            .absolute()
            .top(y)
            .right(px(right - left) - x)
            .gap(ICON_GAP)
            .child(div().size(LEGEND_DOT).rounded_full().bg(theme.primary))
            .child(div().text_size(FONT_SIZE_SMALL).font_weight(WEIGHT_LABEL).child("Now"))
    });
    let marks =
        (!look.marks.is_empty()).then(|| {
            let (x, y) = hang(-tilt.half_width + pad);
            div().absolute().top(y).left(x).flex().gap(ICON_GAP).children(look.marks.into_iter().map(|mark| {
                chip().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).font_weight(WEIGHT_LABEL).child(mark)
            }))
        });
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(right - left))
        .h(px(bottom - top))
        .child(shader)
        .children(now)
        .children(marks)
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let from_middle = event.position - corner.get();
            if tilt.covers(f32::from(from_middle.x) - in_box.0, f32::from(from_middle.y) - in_box.1, 0.0) {
                cx.stop_propagation();
                clicked(event, window, cx);
            }
        })
}

/// What `card.wgsl` reads for a card painted over `bounds`, its middle
/// `in_box` from the box's corner, on a window that draws `scale` device
/// pixels per pixel.
fn params(card: &Painted, bounds: Bounds<Pixels>, in_box: (f32, f32), scale: f32) -> [f32; SHADER_PARAMS] {
    // GPUI paints the box from the whole device pixel nearest its corner.
    let from_corner = |corner: Pixels, within: f32| {
        let corner = f32::from(corner);
        (corner + within) * scale - snapped(corner * scale)
    };
    let color = |color: Hsla| {
        let color = Rgba::from(color);
        [color.r, color.g, color.b, color.a]
    };
    let tilt = card.tilt;
    let rows = [
        [
            from_corner(bounds.origin.x, in_box.0),
            from_corner(bounds.origin.y, in_box.1),
            tilt.half_width * scale,
            tilt.half_height * scale,
        ],
        [card.radius * scale, card.ring * scale, tilt.narrow, tilt.lean / scale],
        [card.opacity, card.dim, 0.0, 0.0],
        color(card.dim_color),
        color(card.ring_color),
        color(card.loading),
    ];
    let mut params = [0.0; SHADER_PARAMS];
    for (slot, value) in params.iter_mut().zip(rows.into_iter().flatten()) {
        *slot = value;
    }
    params
}

/// The whole device pixel GPUI snaps a length to: the nearest, a half
/// going toward zero.
fn snapped(length: f32) -> f32 {
    if length >= 0.0 { (length - 0.5).ceil() } else { (length + 0.5).floor() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{black, point, size, white};

    fn painted(tilt: Tilt) -> Painted {
        Painted {
            tilt,
            radius: 14.0,
            ring: 3.0,
            opacity: 0.5,
            dim: 0.28,
            dim_color: black(),
            ring_color: white(),
            loading: white().opacity(0.07),
        }
    }

    #[test]
    fn lengths_reach_the_shader_in_the_screen_s_pixels() {
        let tilt = Tilt::new(400.0, 200.0, 50.0);
        let bounds = Bounds::new(point(px(100.0), px(50.0)), size(px(300.0), px(240.0)));
        let once = params(&painted(tilt), bounds, (160.0, 120.0), 1.0);
        let twice = params(&painted(tilt), bounds, (160.0, 120.0), 2.0);
        assert_eq!(once[..6], [160.0, 120.0, 200.0, 100.0, 14.0, 3.0]);
        assert_eq!(twice[..6], [320.0, 240.0, 400.0, 200.0, 28.0, 6.0]);
        // The turn itself doesn't change; the lean is per pixel, so halves.
        assert_eq!(once[6], twice[6]);
        assert!((once[7] - 2.0 * twice[7]).abs() < 1e-9);
        assert_eq!(once[8..10], [0.5, 0.28]);
    }

    #[test]
    fn the_middle_is_measured_from_the_pixel_the_box_is_painted_from() {
        let tilt = Tilt::new(400.0, 200.0, 0.0);
        // At 1.5 the box's corner (101 -> 151.5) is painted from 151.
        let bounds = Bounds::new(point(px(101.0), px(50.0)), size(px(300.0), px(240.0)));
        let params = params(&painted(tilt), bounds, (160.0, 120.0), 1.5);
        assert_eq!(params[..2], [240.5, 180.0]);
        assert_eq!([snapped(151.5), snapped(151.6), snapped(-0.5)], [151.0, 152.0, 0.0]);
    }
}
