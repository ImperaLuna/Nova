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

const WITH_PICTURE: &str =
    concat!(include_str!("card.wgsl"), "\nfn picture(uv: vec2<f32>) -> vec4<f32> { return image(uv); }\n");
const LOADING_CARD: &str =
    concat!(include_str!("card.wgsl"), "\nfn picture(uv: vec2<f32>) -> vec4<f32> { return quad.params[5]; }\n");

const EDGE: f32 = 1.0;

pub struct CardLook {
    pub picture: Option<Arc<RenderImage>>,
    pub picked: bool,
    pub now: bool,
    pub marks: Vec<String>,
    pub dim: f32,
    pub opacity: f32,
    pub sized: f32,
}

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

fn params(card: &Painted, bounds: Bounds<Pixels>, in_box: (f32, f32), scale: f32) -> [f32; SHADER_PARAMS] {
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
        assert_eq!(once[6], twice[6]);
        assert!((once[7] - 2.0 * twice[7]).abs() < 1e-9);
        assert_eq!(once[8..10], [0.5, 0.28]);
    }

    #[test]
    fn the_middle_is_measured_from_the_pixel_the_box_is_painted_from() {
        let tilt = Tilt::new(400.0, 200.0, 0.0);
        let bounds = Bounds::new(point(px(101.0), px(50.0)), size(px(300.0), px(240.0)));
        let params = params(&painted(tilt), bounds, (160.0, 120.0), 1.5);
        assert_eq!(params[..2], [240.5, 180.0]);
        assert_eq!([snapped(151.5), snapped(151.6), snapped(-0.5)], [151.0, 152.0, 0.0]);
    }
}
