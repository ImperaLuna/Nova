//! The point of the bar's center row that sits at the screen's middle.
//! The central pill's time stays there while workspaces come and go on
//! its left (the pill grows to the left, the time never moves).
//!
//! A view in the center row wraps that thing in [`middle`]. While the frame
//! is laid out, the wrapper records its middle for its window; the stage
//! reads it in the next frame and places the center row so that point is
//! at the screen's middle. It keeps the point's distance from the row's
//! right edge, which stays put while the row grows to the left, so the
//! point never jitters. Without a wrapper the row is centered.

use gpui::{App, Global, IntoElement, ParentElement, Pixels, Styled, Window, WindowId, canvas, div};
use std::collections::HashMap;

/// Each stage window's recorded middle.
#[derive(Default)]
struct Middles(HashMap<WindowId, Mark>);

impl Global for Middles {}

#[derive(Clone, Copy)]
struct Mark {
    at: Pixels,
    /// Recorded in the frame under way; cleared as the stage reads it.
    fresh: bool,
}

/// Keeps `content`'s middle at the screen's middle, for a view in the
/// bar's center row (the clock part's time). One per screen: with several,
/// the last laid out wins.
pub fn middle(content: impl IntoElement) -> impl IntoElement {
    let recorder = canvas(
        |bounds, window, cx| {
            let id = window.window_handle().window_id();
            let at = bounds.center().x;
            let before = cx.default_global::<Middles>().0.insert(id, Mark { at, fresh: true });
            if before.map(|mark| mark.at) != Some(at) {
                // The stage places its row by it in the next frame.
                window.request_animation_frame();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0();
    div().relative().child(content).child(recorder)
}

/// The middle `window`'s last frame recorded, read as a frame starts: none
/// when no wrapper recorded one then.
pub(crate) fn read(window: &Window, cx: &mut App) -> Option<Pixels> {
    let id = window.window_handle().window_id();
    let middles = &mut cx.default_global::<Middles>().0;
    let mark = middles.get_mut(&id)?;
    if mark.fresh {
        mark.fresh = false;
        Some(mark.at)
    } else {
        middles.remove(&id);
        None
    }
}

/// Whether a wrapper recorded a middle in the frame under way, asked once
/// it is laid out: a stage that placed its row by one that is gone draws
/// once more.
pub(crate) fn recorded(window: &Window, cx: &App) -> bool {
    let id = window.window_handle().window_id();
    cx.try_global::<Middles>().and_then(|middles| middles.0.get(&id)).is_some_and(|mark| mark.fresh)
}
