//! Step 5: the lock screen as GPUI session lock surfaces, through the fork
//! patch in fork-patch/2-session-lock.patch (WindowKind::SessionLock). One
//! lock surface per screen; the first one locks the session and closing the
//! last one unlocks it. Typing fills the field with dots and Enter unlocks:
//! the prototype checks no password. In Nova the check would be PAM called
//! from this same process (as Quickshell's lock does today), so the password
//! never leaves it.
//!
//! `--lock --toggles N --every MS` locks, unlocks after N * MS, and quits.
//! Only ever run it inside the headless test compositor.

use std::time::Duration;

use gpui::{
    App, Bounds, Context, FocusHandle, IntoElement, KeyDownEvent, Render, Size, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px,
};

use crate::{FG, FG_DIM_OPACITY, GLASS_TINT, Options, color, day_and_time};

const LOCK_FIELD_HEIGHT: f32 = 44.0;
const LOCK_FIELD_WIDTH: f32 = 300.0;
const LOCK_DOT: f32 = 8.0;
const LOCK_DOT_GAP: f32 = 6.0;
const LOCK_GAP: f32 = 16.0;
const FONT_SIZE_HERO: f32 = 56.0;
const FONT_SIZE_HEADING: f32 = 15.0;
const BACKDROP: u32 = 0x000000;

struct Lock {
    focus: FocusHandle,
    typed: usize,
    offset: i64,
    font: String,
}

impl Lock {
    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" if self.typed > 0 => {
                eprintln!("lock: unlock requested ({} characters, not checked)", self.typed);
                unlock(cx);
            }
            "backspace" => self.typed = self.typed.saturating_sub(1),
            _ if event.keystroke.key_char.is_some() => self.typed += 1,
            _ => {}
        }
        cx.notify();
    }
}

impl Render for Lock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let (day, time) = day_and_time(self.offset);
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .size_full()
            .bg(color(BACKDROP, 1.0))
            .font_family(self.font.clone())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(LOCK_GAP))
            .text_color(color(FG, 1.0))
            .child(div().text_size(px(FONT_SIZE_HERO)).child(time))
            .child(div().text_size(px(FONT_SIZE_HEADING)).text_color(color(FG, FG_DIM_OPACITY)).child(day))
            .child(
                div()
                    .w(px(LOCK_FIELD_WIDTH))
                    .h(px(LOCK_FIELD_HEIGHT))
                    .rounded(px(LOCK_FIELD_HEIGHT / 2.0))
                    .bg(color(GLASS_TINT, 0.9))
                    .border_1()
                    .border_color(color(FG, 0.10))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(LOCK_DOT_GAP))
                    .children(
                        (0..self.typed.min(24)).map(|_| {
                            div().w(px(LOCK_DOT)).h(px(LOCK_DOT)).rounded(px(LOCK_DOT / 2.0)).bg(color(FG, 1.0))
                        }),
                    ),
            )
    }
}

pub fn open(cx: &mut App, options: &Options, offset: i64) {
    for display in cx.displays() {
        let bounds = display.bounds();
        let font = options.font.clone();
        cx.open_window(
            WindowOptions {
                titlebar: None,
                display_id: Some(display.id()),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: Size::new(bounds.size.width, bounds.size.height),
                })),
                app_id: Some("nova-proto".into()),
                inactive_frame_interval: None,
                window_background: WindowBackgroundAppearance::Opaque,
                kind: WindowKind::SessionLock,
                ..Default::default()
            },
            move |_, cx| cx.new(|cx| Lock { focus: cx.focus_handle(), typed: 0, offset, font }),
        )
        .expect("open a lock surface");
    }
    if options.toggles > 0 {
        let wait = options.every * options.toggles;
        cx.spawn(async move |cx| {
            cx.background_executor().timer(wait).await;
            eprintln!("lock: unlocking after {:.1}s", wait.as_secs_f32());
            cx.update(unlock);
        })
        .detach();
    }
}

/// Closes every lock surface (the last one unlocks the session), then quits.
fn unlock(cx: &mut App) {
    for window in cx.windows() {
        window.update(cx, |_, window, _| window.remove_window()).ok();
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(300)).await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}
