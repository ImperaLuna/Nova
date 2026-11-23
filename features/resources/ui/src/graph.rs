//! The resources page's graph: CPU (a line over a filled area, in the
//! accent) and memory (a line, in the second accent) over the history
//! novad keeps, newest at the right, with quiet lines at a quarter, half
//! and three quarters. `Graph.qml` in today's shell.

use crate::logic::graph_points;
use crate::tokens::{GRAPH_AREA_OPACITY, GRAPH_HEIGHT, GRAPH_LINE};
use gpui::{
    App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, RenderOnce, Styled, Window, canvas, fill, point, px,
    size,
};
use nova_look::Theme;
use nova_look::tokens::DIVIDER_WIDTH;
use nova_resources_api::ResourceSample;

#[derive(IntoElement)]
pub struct Graph {
    history: Vec<ResourceSample>,
    /// How many samples fill the width.
    span: usize,
}

impl Graph {
    pub fn new(history: Vec<ResourceSample>, span: usize) -> Self {
        Self { history, span }
    }
}

/// `values` as points inside `bounds`.
fn points(values: &[f32], bounds: Bounds<Pixels>, span: usize) -> Vec<Point<Pixels>> {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    graph_points(values, width, height, span).into_iter().map(|(x, y)| bounds.origin + point(px(x), px(y))).collect()
}

fn line(points: &[Point<Pixels>], color: Hsla, window: &mut Window) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(GRAPH_LINE);
    path.add_polygon(points, false);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The area under `points`, down to the graph's bottom.
fn area(points: &[Point<Pixels>], bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return };
    if points.len() < 2 {
        return;
    }
    let bottom = bounds.bottom();
    let mut outline = points.to_vec();
    outline.push(point(last.x, bottom));
    outline.push(point(first.x, bottom));
    let mut path = PathBuilder::fill();
    path.add_polygon(&outline, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

impl RenderOnce for Graph {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (primary, secondary, outline) = (theme.primary, theme.secondary, theme.outline());
        let cpu: Vec<f32> = self.history.iter().map(|sample| sample.cpu).collect();
        let memory: Vec<f32> = self.history.iter().map(|sample| sample.memory).collect();
        let span = self.span;
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, _, window, _| {
                for share in [0.25, 0.5, 0.75] {
                    let y = bounds.top() + bounds.size.height * share;
                    let rule = Bounds::new(point(bounds.left(), y), size(bounds.size.width, DIVIDER_WIDTH));
                    window.paint_quad(fill(rule, outline));
                }
                let cpu = points(&cpu, bounds, span);
                area(&cpu, bounds, primary.alpha(GRAPH_AREA_OPACITY), window);
                line(&cpu, primary, window);
                line(&points(&memory, bounds, span), secondary, window);
            },
        )
        .w_full()
        .h(GRAPH_HEIGHT)
    }
}
