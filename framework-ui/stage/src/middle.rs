use gpui::{App, Global, IntoElement, ParentElement, Pixels, Styled, Window, WindowId, canvas, div};
use std::collections::HashMap;

#[derive(Default)]
struct Middles(HashMap<WindowId, Mark>);

impl Global for Middles {}

#[derive(Clone, Copy)]
struct Mark {
    at: Pixels,
    fresh: bool,
}

pub fn middle(content: impl IntoElement) -> impl IntoElement {
    let recorder = canvas(
        |bounds, window, cx| {
            let id = window.window_handle().window_id();
            let at = bounds.center().x;
            let before = cx.default_global::<Middles>().0.insert(id, Mark { at, fresh: true });
            if before.map(|mark| mark.at) != Some(at) {
                window.request_animation_frame();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0();
    div().relative().child(content).child(recorder)
}

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

pub(crate) fn recorded(window: &Window, cx: &App) -> bool {
    let id = window.window_handle().window_id();
    cx.try_global::<Middles>().and_then(|middles| middles.0.get(&id)).is_some_and(|mark| mark.fresh)
}
