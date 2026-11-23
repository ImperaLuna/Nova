//! Nova's loading spinner: an arc going round a ring. `Spinner.qml` in
//! today's shell.
//!
//! The same ring, dashed and with no arc, marks something that isn't there
//! (the AI usage pill of a tool with no numbers). The spinner asks for no
//! frames itself: whoever draws a spinning one asks for the next while it
//! is in sight, and [`Spinner::turned`] says where the arc is by then.

use crate::Theme;
use crate::tokens::{DURATION_SPIN, ICON_SIZE, RING_DASH, RING_INSET, SPINNER_SWEEP, STROKE_THIN};
use gpui::{App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, RenderOnce, Styled, Window, canvas, point};
use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Instant;

/// When the first spinner turned: every spinner turns from it, so two in
/// sight turn together.
static TURNING_SINCE: OnceLock<Instant> = OnceLock::new();

#[derive(Debug, Clone, Copy, IntoElement)]
pub struct Spinner {
    size: Pixels,
    color: Option<Hsla>,
    track: Option<Hsla>,
    dashed: bool,
    spinning: bool,
}

impl Spinner {
    /// A spinning arc in the theme's ink, at the panel icon size, with no
    /// ring under it.
    pub fn new() -> Self {
        Self { size: ICON_SIZE, color: None, track: None, dashed: false, spinning: true }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// The whole ring under the arc, in `color`: a ring that stays when it
    /// stops spinning.
    pub fn track(mut self, color: Hsla) -> Self {
        self.track = Some(color);
        self
    }

    /// The ring drawn in dashes.
    pub fn dashed(mut self, dashed: bool) -> Self {
        self.dashed = dashed;
        self
    }

    /// `false` draws the ring alone.
    pub fn spinning(mut self, spinning: bool) -> Self {
        self.spinning = spinning;
        self
    }

    /// How far round the arc is at `now`, as a share of a turn.
    pub fn turned(now: Instant) -> f32 {
        turned_since(*TURNING_SINCE.get_or_init(|| now), now)
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

fn turned_since(since: Instant, now: Instant) -> f32 {
    let spin = DURATION_SPIN.as_secs_f32();
    now.saturating_duration_since(since).as_secs_f32() % spin / spin
}

/// The points along a circle round `center` from `start` to `end` (radians,
/// clockwise from three o'clock as the screen's y runs down), close enough
/// that the lines between them read as the arc.
pub(crate) fn arc(center: Point<Pixels>, radius: Pixels, start: f32, end: f32) -> Vec<Point<Pixels>> {
    // A point every few degrees: smooth at icon sizes.
    const STEPS_PER_TURN: f32 = 96.0;
    let steps = ((end - start).abs() / TAU * STEPS_PER_TURN).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|step| {
            let angle = start + (end - start) * step as f32 / steps as f32;
            point(center.x + radius * angle.cos(), center.y + radius * angle.sin())
        })
        .collect()
}

fn stroke(points: &[Point<Pixels>], color: Hsla, dashes: Option<&[Pixels]>, window: &mut Window) {
    let mut path = PathBuilder::stroke(STROKE_THIN);
    if let Some(dashes) = dashes {
        path = path.dash_array(dashes);
    }
    path.add_polygon(points, false);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(Theme::of(cx).fg);
        let turned = self.spinning.then(|| Spinner::turned(Instant::now()));
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, _, window, _| {
                let center = bounds.center();
                let radius = bounds.size.width.min(bounds.size.height) / 2.0 - RING_INSET;
                if let Some(track) = self.track {
                    let dash = STROKE_THIN * RING_DASH;
                    let dashes = [dash, dash];
                    stroke(&arc(center, radius, 0.0, TAU), track, self.dashed.then_some(&dashes[..]), window);
                }
                if let Some(turned) = turned {
                    let start = turned * TAU;
                    stroke(&arc(center, radius, start, start + SPINNER_SWEEP.to_radians()), color, None, window);
                }
            },
        )
        .size(self.size)
        .flex_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;
    use std::time::Duration;

    #[test]
    fn it_goes_round_once_a_spin() {
        let start = Instant::now();
        let quarter = turned_since(start, start + DURATION_SPIN / 4);
        let whole = turned_since(start, start + DURATION_SPIN * 3 + Duration::from_millis(1));
        assert!(quarter > 0.2 && quarter <= 0.3, "{quarter}");
        assert!(whole < 0.01, "{whole}");
    }

    #[test]
    fn the_arc_runs_along_the_circle() {
        let points = arc(point(px(8.0), px(8.0)), px(6.0), 0.0, TAU / 4.0);
        let (first, last) = (points[0], points[points.len() - 1]);
        assert!((first.x - px(14.0)).abs() < px(0.01) && (first.y - px(8.0)).abs() < px(0.01), "{first:?}");
        // A quarter turn clockwise on screen: straight down.
        assert!((last.x - px(8.0)).abs() < px(0.01) && (last.y - px(14.0)).abs() < px(0.01), "{last:?}");
    }
}
