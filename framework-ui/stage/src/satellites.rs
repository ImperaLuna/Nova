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

fn bar_room() -> Pixels {
    BAR_GAP + BAR_HEIGHT + BAR_GAP
}

struct Catcher {
    screen: Size<Pixels>,
    stage_height: Rc<Cell<Pixels>>,
    beside: Rc<Cell<Pixels>>,
}

impl Render for Catcher {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        set_blur_region(window, &[]);
        window.set_input_region(Some(&[below_stage(self.stage_height.get(), self.beside.get(), self.screen)]));
        div().size_full().on_any_mouse_down(|_, _, cx| Stage::dismiss(cx)).on_scroll(|_, _, cx| Stage::dismiss(cx))
    }
}

pub(crate) fn open_catcher(
    screen: &Screen,
    stage_height: Rc<Cell<Pixels>>,
    beside: Rc<Cell<Pixels>>,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    let mut shell = ShellWindow::new(screen.id, screen.size);
    shell.layer = Layer::Overlay;
    shell.anchor = Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT;
    shell.namespace = CATCHER_NAMESPACE;
    let size = screen.size;
    opened(
        cx.open_window(shell.options(), move |_, cx| cx.new(|_| Catcher { screen: size, stage_height, beside })),
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

fn kept_room(kept: bool) -> Pixels {
    if kept { bar_room() } else { px(0.0) }
}

pub(crate) fn open_bar_room(screen: &Screen, kept: bool, cx: &mut App) -> Option<AnyWindowHandle> {
    let mut shell = ShellWindow::new(screen.id, size(screen.size.width, BAR_ROOM_HEIGHT));
    shell.exclusive_zone = kept_room(kept);
    shell.namespace = BAR_ROOM_NAMESPACE;
    opened(cx.open_window(shell.options(), |_, cx| cx.new(|_| BarRoom)), "bar's room")
}

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

pub(crate) fn close(window: AnyWindowHandle, cx: &mut App) {
    let _ = window.update(cx, |_, window, _| window.remove_window());
}
