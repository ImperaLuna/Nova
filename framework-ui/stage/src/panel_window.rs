//! The window beside a screen's stage for the panels placed off the top
//! strip (`Placement`): a drawer against the right edge, a panel over the
//! whole screen. On the strip they would make the stage's window as tall
//! as the screen for as long as their feature is on. `PanelSurface.qml` in
//! today's shell.
//!
//! The stage's view opens it when a feature with such a panel becomes
//! ready, and it stays: a new window holds every window's frames up for a
//! moment, which must never be the start of a panel's morph. While none of
//! its panels is out it is parked, one pixel in the screen's top right
//! corner with nothing to blur and no clicks to take, and draws no frames.
//! When one opens it grows to what that panel draws (and its shadow), and
//! it parks again once the last has closed.
//!
//! It keeps no panel itself. The stage's view has every panel of its
//! screen, with its morph and its size, so the rules, the pills and
//! `Stage::resize` are one code path; it writes the ones placed off the
//! strip where this window reads them each frame (`OffStripPanels`), and
//! this window draws them as the strip would (`draw::panel`), shifted by
//! where the window sits on the screen. It never reads the stage's view
//! while it draws: GPUI would count the view as this window's. The
//! rest of the contract is kept here for them: the blur and click regions
//! on what is drawn, the keyboard while one is open, Esc, and a click off
//! them.
//!
//! A drawer grows out of a pill that the strip draws, in another window,
//! and two windows never draw in the same frame. So the pill stays until
//! this window has painted the drawer over it, and the closed drawer stays
//! here, in the pill's shape, until the strip has painted the pill again
//! (`PanelItem::cover`): glass twice for a frame, and never a gap.

use crate::draw::{self, ESCAPE, Shapes, dismiss_outside};
use crate::frames::FrameLog;
use crate::geometry::edge_window;
use crate::item::Placement;
use crate::screens::Screen;
use crate::stage::Stage;
use crate::tokens::{PANEL_NAMESPACE, PANEL_WINDOW_PARKED};
use crate::view::{OffStripPanels, PanelItem, StageView};
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

/// What the window holds for its panels, as the stage's view tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Hold {
    /// One of its panels is open and closes on Esc or takes typing.
    pub keyboard: bool,
    /// A panel that a click outside closes is open, on any screen.
    pub catching: bool,
}

/// What the stage's view tells the window when its panels changed.
pub(crate) struct Told {
    pub hold: Hold,
    /// The field that gets the keyboard when a panel opened here (`None`
    /// inside for the window's own).
    pub focus: Option<Option<FocusHandle>>,
    /// Whether one of its panels heads elsewhere now.
    pub moved: bool,
}

/// The glass, the boxes without glass, whether clicks were caught
/// everywhere and whether it was parked, as the compositor was last told.
type Sent = (Vec<Shape>, Vec<Shape>, bool, bool);

/// What the window shares with the stage's view that opened it.
pub(crate) struct Shared {
    /// The view, to tell it of a change. Gone only with this window.
    pub stage: WeakEntity<StageView>,
    /// The panels to draw.
    pub panels: OffStripPanels,
    /// Where the window says how wide it is.
    pub beside: Rc<Cell<Pixels>>,
}

pub(crate) struct PanelWindow {
    stage: WeakEntity<StageView>,
    panels: OffStripPanels,
    /// The size of the screen it is on.
    screen: Size<Pixels>,
    /// The glass drawn in the frame under way, and the boxes of panels
    /// without glass, which take clicks and get no blur.
    glass: Shapes,
    plain: Shapes,
    sent: Rc<RefCell<Option<Sent>>>,
    hold: Hold,
    /// Gets the keyboard for a panel that names no field of its own, so Esc
    /// arrives.
    focus: FocusHandle,
    frames: Option<FrameLog>,
    /// The size the window was last asked to be.
    size: Size<Pixels>,
    /// How wide it is on screen, nothing while parked: the stage and the
    /// click catcher leave it the clicks there.
    beside: Rc<Cell<Pixels>>,
}

fn parked() -> Size<Pixels> {
    size(PANEL_WINDOW_PARKED, PANEL_WINDOW_PARKED)
}

/// Opens the window on `screen`, parked.
pub(crate) fn open(
    screen: &Screen,
    shared: Shared,
    log_frames: bool,
    cx: &mut App,
) -> Option<WindowHandle<PanelWindow>> {
    let Shared { stage, panels, beside } = shared;
    let screen_size = screen.size;
    let mut shell = ShellWindow::new(screen.id, parked());
    // Over a fullscreen window too, and from the start: Hyprland ignored a
    // change of layer before a first frame (today's PanelSurface).
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

/// What a frame draws, worked out from the stage's panels.
struct Drawn {
    panels: Vec<AnyElement>,
    /// Whether one of them is on its way.
    moving: bool,
    /// The size the window needs for them; `None` with nothing to draw.
    needs: Option<Size<Pixels>>,
    /// Each drawn panel's mark that it is on screen here.
    covers: Vec<Rc<Cell<bool>>>,
    /// Whether one has closed and waits for the strip to draw its pill.
    handing_back: bool,
}

/// The size a window needs to draw `panel`: its drawer and the room for
/// its shadow, at the width it is heading for or the one it still has; or
/// the screen.
fn needs(panel: &PanelItem, screen: Size<Pixels>, now: Instant) -> Size<Pixels> {
    match panel.spec.placement {
        Placement::WholeScreen => screen,
        Placement::RightEdge | Placement::Top => {
            edge_window(px(panel.width.target().max(panel.width.value(now))), screen)
        }
    }
}

impl PanelWindow {
    /// Takes what the stage's view tells it.
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
        // Parked, catching clicks elsewhere is nothing to draw for.
        let out = self.beside.get() > px(0.0);
        if out || moved || keyboard_changed || focus.is_some() {
            cx.notify();
        }
    }

    /// Lays out and draws the panels placed off the strip, for a window
    /// `viewport` big.
    fn draw(&self, viewport: Size<Pixels>, now: Instant, cx: &App) -> Drawn {
        let screen = self.screen;
        // The window sits in the screen's top right corner.
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
            let needs = needs(panel, screen, now);
            let widest = drawn.needs.map_or(needs.width, |so_far| so_far.width.max(needs.width));
            drawn.needs = Some(size(widest, needs.height));
        }
        drawn
    }

    /// Makes the window as big as what is drawn in it needs: it grows at
    /// once, before the panel does, keeps its size while one is `moving`
    /// (to a smaller, or closing while the next opens), shrinks to what is
    /// left once all stand, and parks with nothing to draw.
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

    /// Has the stage's view draw again, and leave this window its clicks:
    /// it changed size, or has a panel on screen for the first time, or
    /// waits to hand a closed one's pill back.
    fn tell_stage(&self, cx: &mut App) {
        let stage = self.stage.clone();
        cx.defer(move |cx| {
            let _ = stage.update(cx, |stage, cx| stage.panel_window_changed(cx));
        });
    }

    /// Hands this frame's shapes to the compositor, when they changed, as
    /// the strip does; and marks the panels as on screen here once the
    /// window is as big as they need (`ready`), which a resize is only a
    /// round trip to the compositor later.
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
