//! One screen's pop-up window: on the overlay layer at the top right,
//! taking no keyboard and no space, only as big as the stack plus its
//! shadow, with blur and clicks only on the cards. Only the one on the
//! screen the pop-ups came out on draws at all, and it asks for frames only
//! while a card moves. It changes size while nothing moves: it grows before
//! new cards open and shrinks once those leaving have gone, since a resize
//! in a motion costs that motion frames.

use crate::card::{Look, card};
use crate::popups::{Heights, Popups, Reach};
use crate::stack::Entry;
use crate::tokens::{ACTION_HEIGHT, AVATAR, COUNTDOWN_TICK, GROW_WAIT, PAD_BOTTOM, WIDTH};
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, IntoElement, ParentElement, Pixels, Render, Styled,
    Subscription, Task, WeakEntity, Window, WindowHandle, canvas, div,
    layer_shell::{Anchor, Layer},
    px, size,
};
use nova_look::tokens::{BAR_MARGIN, DIVIDER_WIDTH, FONT_SIZE_SMALL, GAP, PAD, SHADOW_ROOM};
use nova_look::{LookStyled, Shape, ShellWindow, Theme, region, set_blur_region};
use nova_stage::{FrameLog, Screen, Stage};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The pop-ups' layer surfaces, for the compositor's rules (niri blurs
/// `^nova-`).
const NAMESPACE: &str = "nova-notifications";
/// A window with no pop-ups: as small as the compositor takes.
const RESTING_HEIGHT: Pixels = px(1.0);

pub struct PopupWindow {
    screen: Screen,
    popups: WeakEntity<Popups>,
    heights: Heights,
    /// The shapes drawn this frame, and those the compositor has.
    shapes: Rc<RefCell<Vec<Shape>>>,
    sent: Rc<RefCell<Option<Vec<Shape>>>>,
    /// The height it last asked for, and when.
    height: Pixels,
    resized: Option<Instant>,
    /// The stack is this screen's to draw, as the pop-ups last said.
    shows: bool,
    /// Wakes it for the draining lines' next step.
    countdown: Option<Task<()>>,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

/// The window's width: the stack, its shadow's room on the left, and the
/// bar's margin on the right.
fn window_width() -> Pixels {
    SHADOW_ROOM + WIDTH + BAR_MARGIN
}

impl PopupWindow {
    pub fn open(
        screen: &Screen,
        popups: WeakEntity<Popups>,
        heights: Heights,
        cx: &mut App,
    ) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, size(window_width(), RESTING_HEIGHT));
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::TOP | Anchor::RIGHT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/notifications", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(shell.options(), move |_, cx| {
            cx.new(|cx: &mut Context<Self>| {
                // Whether there's anything to draw is found out here, not
                // while drawing: GPUI redraws a window whenever something
                // its last frame read changes, so a window that read the
                // pop-ups to learn it has none would commit a frame, on a
                // screen that shows no card, each time they change.
                let followed = popups.upgrade().map(|popups| cx.observe(&popups, Self::follow));
                // Pop-ups that came while this window was opening are already
                // waiting for it.
                let shows = popups.upgrade().is_some_and(|popups| popups.read(cx).shows_on(screen.id));
                Self {
                    screen,
                    popups,
                    heights,
                    shapes: Rc::default(),
                    sent: Rc::default(),
                    height: RESTING_HEIGHT,
                    resized: None,
                    shows,
                    countdown: None,
                    frames,
                    _followed: followed,
                }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the notifications' window: {error:#}");
                None
            }
        }
    }

    /// The pop-ups changed: draws when the stack is this screen's, and
    /// once more when it no longer is.
    fn follow(&mut self, popups: Entity<Popups>, cx: &mut Context<Self>) {
        let shows = popups.read(cx).shows_on(self.screen.id);
        if shows || self.shows {
            self.shows = shows;
            cx.notify();
        }
    }

    /// Makes the window `height` tall. Only for a frame in which nothing
    /// moves.
    fn fit(&mut self, height: Pixels, now: Instant, window: &mut Window) {
        let height = if height < RESTING_HEIGHT { RESTING_HEIGHT } else { height.ceil() };
        if height == self.height {
            return;
        }
        self.height = height;
        self.resized = Some(now);
        window.resize(size(window_width(), height));
    }

    /// The window has the height it last asked for, or the compositor has
    /// had long enough to give it and chose another.
    fn fitted(&self, now: Instant, window: &Window) -> bool {
        window.viewport_size().height == self.height
            || self.resized.is_none_or(|at| now.saturating_duration_since(at) > GROW_WAIT)
    }

    /// Nothing to draw: as small as it gets, with no wake-up left.
    fn rest(&mut self, now: Instant, window: &mut Window) -> Div {
        self.countdown = None;
        self.fit(RESTING_HEIGHT, now, window);
        self.log_frame(false, now);
        div().child(self.regions())
    }

    /// While a card's time drains and nothing moves, the next step of its
    /// line comes [`COUNTDOWN_TICK`] after this frame, whatever drew it, so
    /// no step lands in a motion or right behind one.
    fn keep_countdown(&mut self, stepping: bool, cx: &mut Context<Self>) {
        self.countdown = stepping.then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(COUNTDOWN_TICK).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
    }

    /// Hands this frame's shapes to the compositor when they changed: it
    /// blurs behind them, and clicks land on them alone.
    fn regions(&self) -> impl IntoElement {
        let (shapes, sent) = (self.shapes.clone(), self.sent.clone());
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                let drawn = shapes.take();
                if sent.borrow().as_ref() != Some(&drawn) {
                    set_blur_region(window, &drawn);
                    window.set_input_region(Some(&region(&drawn)));
                    sent.replace(Some(drawn));
                }
            },
        )
        .absolute()
        .size_0()
    }

    /// Records a card's height as laid out; a change asks for a frame, so
    /// the slots below it move.
    fn measure(&self, key: u64) -> impl IntoElement {
        let measured = self.heights.clone();
        canvas(
            move |bounds, window, _| {
                let height = bounds.size.height;
                if measured.borrow_mut().insert(key, height) != Some(height) {
                    window.request_animation_frame();
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0()
    }

    fn log_frame(&mut self, moving: bool, now: Instant) {
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
    }
}

impl Render for PopupWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let Some(popups) = self.popups.upgrade().filter(|_| self.shows) else { return self.rest(now, window) };
        popups.update(cx, |popups, _| popups.step(now));
        let popups_now = popups.read(cx);
        if !popups_now.shows_on(self.screen.id) {
            // The last card went in this frame, which read the pop-ups:
            // one more, which reads nothing, lets go of them.
            self.shows = false;
            window.request_animation_frame();
            return self.rest(now, window);
        }
        let top = popups_now.top(cx);
        let stack = popups_now.stack();
        let (entries, waiting, counting) = (stack.entries().to_vec(), stack.waiting(), stack.counting());
        let (mut moving, arrivals) = (stack.moving(now), stack.arrivals().to_vec());
        let heights = self.heights.borrow().clone();
        let height_of = |entry: &Entry| heights.get(&entry.key).copied().unwrap_or(AVATAR + PAD + PAD_BOTTOM);

        // The room every card takes once in place, counting those not
        // opened yet and those leaving, so the window keeps one size
        // through a motion. New ones are laid out unseen first, and wait
        // for the cards on their way to arrive: then the window grows
        // once, to their real heights, and they open in the frame after
        // it has.
        let settled = !moving && arrivals.iter().all(|key| heights.contains_key(key));
        if settled {
            let chip_room = if waiting > 0 { ACTION_HEIGHT } else { px(0.0) };
            let full = entries.iter().fold(chip_room, |total, entry| total + height_of(entry) + GAP);
            self.fit(top + full + SHADOW_ROOM, now, window);
        }
        if !arrivals.is_empty() {
            if settled && self.fitted(now, window) {
                popups.update(cx, |popups, _| popups.open_arrivals(now));
                moving = true;
            } else {
                window.request_animation_frame();
            }
        }
        self.keep_countdown(counting && !moving, cx);

        let wall = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs().cast_signed());
        let mut slot_top = top;
        let mut looks = Vec::new();
        for entry in &entries {
            let height = height_of(entry);
            looks.push(Look { entry, presence: entry.presence.value(now), height, slot_top, now, wall });
            slot_top += (height + GAP) * entry.room.value(now);
        }
        // Those growing or leaving lie over those in place.
        looks.sort_by(|a, b| b.presence.total_cmp(&a.presence));
        let reach = Reach::new(self.popups.clone(), self.screen.name.clone());
        let drawn: Vec<AnyElement> = looks
            .iter()
            .map(|look| {
                if look.presence > 0.0 {
                    let (bounds, radius) = look.bounds();
                    self.shapes.borrow_mut().push(Shape::new(bounds, radius));
                }
                card(look, &reach, self.measure(look.entry.key), cx)
            })
            .collect();
        let chip = (waiting > 0).then(|| chip(waiting, slot_top, cx));
        if moving {
            window.request_animation_frame();
        }
        self.log_frame(moving, now);
        div().size_full().body_text(cx).children(drawn).children(chip).child(self.regions())
    }
}

/// "+N": those waiting for room, under the stack.
fn chip(waiting: usize, top: Pixels, cx: &App) -> impl IntoElement {
    let theme = Theme::of(cx);
    div().absolute().top(top).left(SHADOW_ROOM).w(WIDTH).flex().justify_center().child(
        div()
            .h(ACTION_HEIGHT)
            .px(PAD)
            .flex()
            .items_center()
            .rounded_full()
            .bg(theme.surface())
            .border(DIVIDER_WIDTH)
            .border_color(theme.glass_rim())
            .text_size(FONT_SIZE_SMALL)
            .text_color(theme.fg_soft())
            .child(format!("+{waiting}")),
    )
}
