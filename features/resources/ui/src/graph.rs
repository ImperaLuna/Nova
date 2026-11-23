//! The resources page's graph: CPU (a line over a filled area, in the accent)
//! and memory (a line, in the second accent) over the history novad keeps,
//! newest at the right, with quiet lines at a quarter, half and three quarters.

use crate::tokens::{GRAPH_AREA_OPACITY, GRAPH_HEIGHT};
use gpui::{App, Bounds, IntoElement, Pixels, RenderOnce, Styled, Window, canvas, fill, point, size};
use nova_look::Theme;
use nova_look::graph::{area, line, points};
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
