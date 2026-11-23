//! One screen's pop-up window: on the overlay layer at the top right,
//! taking no keyboard and no space, only as big as the stack plus its
//! shadow, with blur and clicks only on the cards. It draws the stack only
//! on the screen the pop-ups came out on, and asks for frames only while a
//! card moves.

use crate::card::{Look, card};
use crate::popups::{Heights, Popups, Reach};
use crate::stack::Entry;
use crate::tokens::{ACTION_HEIGHT, AVATAR, PAD_BOTTOM, WIDTH};
use gpui::{
    AnyElement, App, AppContext, Context, IntoElement, ParentElement, Pixels, Render, Styled, Subscription, WeakEntity,
    Window, WindowHandle, canvas, div,
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
    height: Pixels,
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
                let followed = popups.upgrade().map(|popups| cx.observe(&popups, |_, _, cx| cx.notify()));
                Self {
                    screen,
                    popups,
                    heights,
                    shapes: Rc::default(),
                    sent: Rc::default(),
                    height: RESTING_HEIGHT,
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

    fn fit(&mut self, height: Pixels, window: &mut Window) {
        let height = if height < RESTING_HEIGHT { RESTING_HEIGHT } else { height.ceil() };
        if height != self.height {
            self.height = height;
            window.resize(size(window_width(), height));
        }
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
        let Some(popups) = self.popups.upgrade() else { return div().child(self.regions()) };
        popups.update(cx, |popups, cx| popups.step(now, cx));
        let popups_now = popups.read(cx);
        if popups_now.target() != Some(self.screen.id) || !popups_now.stack().busy() {
            self.fit(RESTING_HEIGHT, window);
            self.log_frame(false, now);
            return div().child(self.regions());
        }
        let top = popups_now.top(cx);
        let stack = popups_now.stack();
        let (entries, waiting, moving) = (stack.entries().to_vec(), stack.waiting(), stack.moving(now));
        let heights = self.heights.borrow().clone();
        let height_of = |entry: &Entry| heights.get(&entry.key).copied().unwrap_or(AVATAR + PAD + PAD_BOTTOM);

        // The room every card takes once in place, counting those growing
        // in or leaving, so the window keeps one size through the motion.
        let chip_room = if waiting > 0 { ACTION_HEIGHT } else { px(0.0) };
        let full = entries.iter().fold(chip_room, |total, entry| total + height_of(entry) + GAP);
        self.fit(top + full + SHADOW_ROOM, window);

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
