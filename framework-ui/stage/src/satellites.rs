//! The two windows a screen's stage keeps beside itself. Neither draws
//! anything.
//!
//! - The click catcher: while a panel that closes on a click outside is
//!   open, on any screen, it covers the screen below the stage and a click
//!   or a scroll on it closes the panel. The stage is only a strip across
//!   the top, so without it a click further down would go to the window
//!   there. It sits on the overlay layer, over a fullscreen window too.
//! - The bar's room: an empty surface that reserves the bar's height at
//!   the top of the screen, so app windows sit below the pills. The stage
//!   itself reserves nothing, so a panel opening never moves a window. It
//!   gives the room back while the bar steps aside for a fullscreen window
//!   (`crate::aside`), without closing: a new window would cost frames.

use crate::geometry::below_stage;
use crate::screens::Screen;
use crate::stage::Stage;
use crate::tokens::{BAR_ROOM_HEIGHT, BAR_ROOM_NAMESPACE, CATCHER_NAMESPACE};
use gpui::{
    AnyWindowHandle, App, AppContext, Context, InteractiveElement, IntoElement, Pixels, Render, Size, Styled, Window,
    div,
    layer_shell::{Anchor, Layer},
    px, size,
};
use nova_look::tokens::{BAR_GAP, BAR_HEIGHT};
use nova_look::{ShellWindow, set_blur_region};
use std::cell::Cell;
use std::rc::Rc;

/// The room the bar keeps free of app windows: the same gap above and
/// below the pills.
fn bar_room() -> Pixels {
    BAR_GAP + BAR_HEIGHT + BAR_GAP
}

struct Catcher {
    screen: Size<Pixels>,
    /// How tall the stage above is, which takes its own clicks.
    stage_height: Rc<Cell<Pixels>>,
}

impl Render for Catcher {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        set_blur_region(window, &[]);
        window.set_input_region(Some(&[below_stage(self.stage_height.get(), self.screen)]));
        div().size_full().on_any_mouse_down(|_, _, cx| Stage::dismiss(cx)).on_scroll(|_, _, cx| Stage::dismiss(cx))
    }
}

/// Opens the click catcher on `screen`, under a stage that says how tall it
/// is through `stage_height`.
pub(crate) fn open_catcher(screen: &Screen, stage_height: Rc<Cell<Pixels>>, cx: &mut App) -> Option<AnyWindowHandle> {
    let mut shell = ShellWindow::new(screen.id, screen.size);
    shell.layer = Layer::Overlay;
    shell.anchor = Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT;
    shell.namespace = CATCHER_NAMESPACE;
    let size = screen.size;
    opened(
        cx.open_window(shell.options(), move |_, cx| cx.new(|_| Catcher { screen: size, stage_height })),
        "click catcher",
    )
}

struct BarRoom;

impl Render for BarRoom {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        set_blur_region(window, &[]);
        window.set_input_region(Some(&[]));
        div()
    }
}

/// The room the bar's surface keeps: all of it, or none.
fn kept_room(kept: bool) -> Pixels {
    if kept { bar_room() } else { px(0.0) }
}

/// Opens the surface that keeps the bar's room on `screen`, or keeps none
/// yet when `kept` is false.
pub(crate) fn open_bar_room(screen: &Screen, kept: bool, cx: &mut App) -> Option<AnyWindowHandle> {
    let mut shell = ShellWindow::new(screen.id, size(screen.size.width, BAR_ROOM_HEIGHT));
    shell.exclusive_zone = kept_room(kept);
    shell.namespace = BAR_ROOM_NAMESPACE;
    opened(cx.open_window(shell.options(), |_, cx| cx.new(|_| BarRoom)), "bar's room")
}

/// Keeps the bar's room, or gives it back to the windows. The change goes
/// out with the surface's next frame, so it asks for one.
pub(crate) fn keep_bar_room(room: AnyWindowHandle, kept: bool, cx: &mut App) {
    let _ = room.update(cx, |_, window, _| {
        window.set_exclusive_zone(kept_room(kept));
        window.refresh();
    });
}

fn opened<V: 'static>(window: gpui::Result<gpui::WindowHandle<V>>, what: &str) -> Option<AnyWindowHandle> {
    match window {
        Ok(window) => Some(window.into()),
        Err(error) => {
            eprintln!("ERROR nova-ui: can't open the {what}: {error:#}");
            None
        }
    }
}

/// Closes a window opened here. It may be gone already, with its screen.
pub(crate) fn close(window: AnyWindowHandle, cx: &mut App) {
    let _ = window.update(cx, |_, window, _| window.remove_window());
}
