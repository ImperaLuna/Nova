use crate::draw::{self, ESCAPE, Shapes, dismiss_outside};
use crate::frames::FrameLog;
use crate::geometry::{edge_window, edge_window_from};
use crate::item::Placement;
use crate::screens::Screen;
use crate::stage::Stage;
use crate::tokens::{PANEL_NAMESPACE, PANEL_WINDOW_PARKED};
use crate::view::{OffStrip, OffStripPanels, PanelItem, StageView};
use gpui::{
    AnyElement, App, AppContext, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Pixels, Render, Size, Styled, WeakEntity, Window, WindowHandle, canvas, div,
    layer_shell::{Anchor, KeyboardInteractivity, Layer},
    point, px, size,
};
use nova_look::{LookStyled, Shape, ShellWindow, region, set_blur_region};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Hold {
    pub keyboard: bool,
    pub catching: bool,
}

pub(crate) struct Told {
    pub hold: Hold,
    pub focus: Option<Option<FocusHandle>>,
    pub moved: bool,
}

type Sent = (Vec<Shape>, Vec<Shape>, bool, bool);

pub(crate) struct Shared {
    pub stage: WeakEntity<StageView>,
    pub panels: OffStripPanels,
    pub beside: Rc<Cell<Pixels>>,
}

pub(crate) struct PanelWindow {
    stage: WeakEntity<StageView>,
    panels: OffStripPanels,
    screen: Size<Pixels>,
    glass: Shapes,
    plain: Shapes,
    sent: Rc<RefCell<Option<Sent>>>,
    hold: Hold,
    focus: FocusHandle,
    frames: Option<FrameLog>,
    size: Size<Pixels>,
    beside: Rc<Cell<Pixels>>,
}

fn parked() -> Size<Pixels> {
    size(PANEL_WINDOW_PARKED, PANEL_WINDOW_PARKED)
}

pub(crate) fn open(
    screen: &Screen,
    shared: Shared,
    log_frames: bool,
    cx: &mut App,
) -> Option<WindowHandle<PanelWindow>> {
    let Shared { stage, panels, beside } = shared;
    let screen_size = screen.size;
    let mut shell = ShellWindow::new(screen.id, parked());
    // Hyprland ignored a change of layer before a surface's first frame, so the layer is set from the start:
    // https://linear.app/imperaluna/issue/IMP-427
    shell.layer = Layer::Overlay;
    shell.anchor = Anchor::TOP | Anchor::RIGHT;
    shell.namespace = PANEL_NAMESPACE;
    let frames = log_frames.then(|| FrameLog::new(format!("{}/panels", screen.name), screen.refresh, Instant::now()));
    beside.set(px(0.0));
    let opened = cx.open_window(shell.options(), move |_, cx| {
        cx.new(|cx| PanelWindow {
            stage,
            panels,
            screen: screen_size,
            glass: Rc::default(),
            plain: Rc::default(),
            sent: Rc::default(),
            hold: Hold::default(),
            focus: cx.focus_handle(),
            frames,
            size: parked(),
            beside,
        })
    });
    match opened {
        Ok(window) => Some(window),
        Err(error) => {
            eprintln!("ERROR nova-ui: can't open the window for panels off the strip: {error:#}");
            None
        }
    }
}

struct Drawn {
    panels: Vec<AnyElement>,
    moving: bool,
    needs: Option<Size<Pixels>>,
    covers: Vec<Rc<Cell<bool>>>,
    handing_back: bool,
}

fn needs(off_strip: &OffStrip, screen: Size<Pixels>, now: Instant) -> Size<Pixels> {
    let panel = &off_strip.panel;
    match panel.spec.placement {
        Placement::WholeScreen => screen,
        Placement::RightEdge | Placement::RightTab | Placement::Top => {
            let width = px(panel.width.target().max(panel.width.value(now)));
            let stands_open = panel.morph.target() > 0.0 && panel.morph.settled(now);
            match off_strip.pill_box().filter(|_| !stands_open) {
                Some((pill, _)) => edge_window_from(width, pill.left(), screen),
                None => edge_window(width, screen),
            }
        }
    }
}

impl PanelWindow {
    pub fn set_stage(&mut self, stage: WeakEntity<StageView>) {
        self.stage = stage;
    }

    pub fn set_screen(&mut self, screen: Size<Pixels>, cx: &mut Context<Self>) {
        if self.screen != screen {
            self.screen = screen;
            cx.notify();
        }
    }

    pub fn follow(&mut self, told: Told, window: &mut Window, cx: &mut Context<Self>) {
        let Told { hold, focus, moved } = told;
        let keyboard_changed = hold.keyboard != self.hold.keyboard;
        if keyboard_changed {
            let keyboard = if hold.keyboard { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None };
            window.set_keyboard_interactivity(keyboard);
        }
        self.hold = hold;
        if let Some(focus) = &focus {
            window.focus(focus.as_ref().unwrap_or(&self.focus), cx);
        }
        let out = self.beside.get() > px(0.0);
        if out || moved || keyboard_changed || focus.is_some() {
            cx.notify();
        }
    }

    fn draw(&self, viewport: Size<Pixels>, now: Instant, cx: &App) -> Drawn {
        let screen = self.screen;
        let origin = point(screen.width - viewport.width, px(0.0));
        let shown = |panel: &PanelItem| !panel.closed(now) || panel.cover.get();
        let mut drawn =
            Drawn { panels: Vec::new(), moving: false, needs: None, covers: Vec::new(), handing_back: false };
        for off_strip in self.panels.borrow().iter().filter(|off_strip| shown(&off_strip.panel)) {
            let panel = &off_strip.panel;
            let frame = draw::frame(panel, off_strip.pill_box(), screen, now);
            let shapes = if frame.glassy { &self.glass } else { &self.plain };
            drawn.panels.push(draw::panel(panel, &frame, origin, shapes, cx));
            drawn.moving |= panel.moving(now);
            drawn.handing_back |= panel.closed(now);
            drawn.covers.push(panel.cover.clone());
            let needs = needs(off_strip, screen, now);
            let widest = drawn.needs.map_or(needs.width, |so_far| so_far.width.max(needs.width));
            drawn.needs = Some(size(widest, needs.height));
        }
        drawn
    }

    fn fit(&mut self, needs: Option<Size<Pixels>>, moving: bool, window: &mut Window, cx: &mut Context<Self>) {
        let out = self.beside.get() > px(0.0);
        let wanted = match needs {
            Some(needs) if out && moving => size(needs.width.max(self.size.width), needs.height),
            Some(needs) => needs,
            None => parked(),
        };
        if wanted == self.size {
            return;
        }
        self.size = wanted;
        self.beside.set(if needs.is_some() { wanted.width } else { px(0.0) });
        window.resize(wanted);
        self.tell_stage(cx);
    }

    fn tell_stage(&self, cx: &mut App) {
        let stage = self.stage.clone();
        cx.defer(move |cx| {
            let _ = stage.update(cx, |stage, cx| stage.panel_window_changed(cx));
        });
    }

    fn regions(&self, covers: Vec<Rc<Cell<bool>>>, ready: bool) -> impl IntoElement + use<> {
        let (glass, plain, sent) = (self.glass.clone(), self.plain.clone(), self.sent.clone());
        let (catching, parked) = (self.hold.catching, self.beside.get() == px(0.0));
        let stage = self.stage.clone();
        canvas(
            |_, _, _| {},
            move |_, (), window, cx| {
                let newly_covered = ready && covers.iter().filter(|cover| !cover.replace(true)).count() > 0;
                if newly_covered {
                    cx.defer(move |cx| {
                        let _ = stage.update(cx, |stage, cx| stage.panel_window_changed(cx));
                    });
                }
                let drawn = (glass.take(), plain.take(), catching, parked);
                let solid = [drawn.0.as_slice(), drawn.1.as_slice()].concat();
                if sent.borrow().as_ref() != Some(&drawn) {
                    set_blur_region(window, &drawn.0);
                    let clicks = region(&solid);
                    let everywhere = catching && !parked;
                    window.set_input_region(if everywhere { None } else { Some(&clicks) });
                    sent.replace(Some(drawn));
                }
                if catching && !parked {
                    dismiss_outside(solid, window);
                }
            },
        )
        .absolute()
        .size_0()
    }
}

impl Render for PanelWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.glass.borrow_mut().clear();
        self.plain.borrow_mut().clear();
        let viewport = window.viewport_size();
        let drawn = self.draw(viewport, now, cx);
        self.fit(drawn.needs, drawn.moving, window, cx);
        if drawn.moving {
            window.request_animation_frame();
        }
        if drawn.handing_back {
            self.tell_stage(cx);
        }
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(drawn.moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
        let ready = drawn.needs.is_some_and(|needs| viewport.width >= needs.width && viewport.height >= needs.height);
        div()
            .id("panels")
            .size_full()
            .body_text(cx)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == ESCAPE {
                    Stage::dismiss(cx);
                }
            }))
            .children(drawn.panels)
            .child(self.regions(drawn.covers, ready))
            .into_any_element()
    }
}
