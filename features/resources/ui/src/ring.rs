use crate::tokens::{RING_SIZE, RING_STROKE};
use gpui::{
    App, Bounds, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, Point, RenderOnce, SharedString, Styled,
    Window, canvas, div, fill, point, size,
};
use nova_look::Theme;
use nova_look::tokens::{FONT_MONO, FONT_SIZE_MICRO, FONT_SIZE_SMALL, ICON_GAP, WEIGHT_LABEL};
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(IntoElement)]
pub struct Ring {
    value: f32,
    reading: SharedString,
    label: SharedString,
}

impl Ring {
    pub fn new(value: f32, reading: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self { value: value.clamp(0.0, 1.0), reading: reading.into(), label: label.into() }
    }
}

fn arc(center: Point<Pixels>, radius: Pixels, sweep: f32) -> Vec<Point<Pixels>> {
    const STEPS_PER_TURN: f32 = 120.0;
    let steps = (sweep * STEPS_PER_TURN).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|step| {
            let angle = -FRAC_PI_2 + TAU * sweep * step as f32 / steps as f32;
            point(center.x + radius * angle.cos(), center.y + radius * angle.sin())
        })
        .collect()
}

fn stroke(points: &[Point<Pixels>], color: Hsla, window: &mut Window) {
    let mut path = PathBuilder::stroke(RING_STROKE);
    path.add_polygon(points, false);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn cap(at: Point<Pixels>, color: Hsla, window: &mut Window) {
    let dot = Bounds::centered_at(at, size(RING_STROKE, RING_STROKE));
    window.paint_quad(fill(dot, color).corner_radii(RING_STROKE / 2.0));
}

impl RenderOnce for Ring {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (track, meter) = (theme.track(), theme.meter());
        let value = self.value;
        let drawn = canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, _, window, _| {
                let center = bounds.center();
                let radius = RING_SIZE / 2.0 - RING_STROKE;
                stroke(&arc(center, radius, 1.0), track, window);
                if value > 0.0 {
                    let points = arc(center, radius, value);
                    stroke(&points, meter, window);
                    for end in [points.first(), points.last()].into_iter().flatten() {
                        cap(*end, meter, window);
                    }
                }
            },
        )
        .absolute()
        .inset_0();
        div()
            .flex_none()
            .w(RING_SIZE)
            .flex()
            .flex_col()
            .items_center()
            .gap(ICON_GAP)
            .child(
                div().relative().size(RING_SIZE).flex().items_center().justify_center().child(drawn).child(
                    div()
                        .font_family(FONT_MONO)
                        .text_size(FONT_SIZE_SMALL)
                        .font_weight(WEIGHT_LABEL)
                        .text_color(theme.fg)
                        .child(self.reading),
                ),
            )
            .child(div().text_size(FONT_SIZE_MICRO).text_color(theme.fg_soft()).child(self.label.to_uppercase()))
    }
}
