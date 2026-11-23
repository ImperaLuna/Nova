//! The tray in the right pill (view `tray.strip`, host `right-pill.tray`),
//! ported from `ui/bar/RightPill.qml`: a chevron while some app sits in the
//! tray, which opens it in place. The pill grows to the left and the apps'
//! icons pop in, at most `TRAY_VISIBLE` of them, the rest a wheel scroll
//! away with a "+N" on each side for those out of sight.
//!
//! Opening the tray is opening the panel `tray` (view `tray.panel`, drawn
//! here, in place): the stage closes it on Esc, a click outside or another
//! panel opening, as it does any panel.

use crate::menu;
use crate::model::{self, Entry};
use crate::tokens::{REVEAL_PILL_COUNT, TRAY_DOT_RING, TRAY_DOT_SIZE, TRAY_ICON_SIZE, TRAY_STAGGER, TRAY_VISIBLE};
use gpui::{
    AnyElement, App, Context, Div, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Render, ScrollEvent, SharedString, Stateful, Styled, Subscription, Window, canvas, div, font,
    prelude::FluentBuilder, px,
};
use nova_look::motion::{Eased, ease};
use nova_look::tokens::{
    BAR_HEIGHT, DURATION_FADE, DURATION_FAST, DURATION_PANEL, ENTER_SCALE, FONT_MONO, FONT_SIZE_MICRO, PILL_BUTTON_GAP,
    PILL_BUTTON_SIZE,
};
use nova_look::{AppIcon, Glyph, Icon, Theme, Wheel};
use nova_stage::{Screen, Shown, Stage};
use nova_tray_api::{Command, Event, TOPIC, Tray};
use nova_ui_client::{Daemon, Live};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The panel the open tray is, as today's shell calls it.
pub const TRAY: &str = "tray";

/// A button's top in the bar, so it sits in the middle.
fn button_top() -> Pixels {
    (BAR_HEIGHT - PILL_BUTTON_SIZE) / 2.0
}

pub struct Strip {
    screen: Screen,
    shown: Shown,
    tray: Entity<Live<Tray>>,
    apps: Vec<Entry>,
    attention: bool,
    open: bool,
    /// When it last opened or closed: the icons pop in one after another
    /// from then, and the counts wait for the pill to have grown.
    toggled: Instant,
    /// Shut 0, open 1.
    morph: Eased,
    /// How far the strip is scrolled, in pixels.
    scroll: Eased,
    wheel: Wheel,
    /// The chevron's attention dot, and each icon's.
    dot: Eased,
    dots: HashMap<String, Eased>,
    counts: [Count; 2],
    /// Each icon's middle across the screen, as last laid out: a menu
    /// drops below it.
    middles: Rc<RefCell<HashMap<String, Pixels>>>,
    _follow: [Option<Subscription>; 2],
}

/// A "+N": it fades out with none out of sight but keeps its room, and
/// shows the last number while it does.
struct Count {
    last: usize,
    shows: Eased,
}

impl Count {
    fn new(now: Instant) -> Self {
        Self { last: 1, shows: Eased::lasting(0.0, DURATION_FAST, now) }
    }

    fn set(&mut self, hidden: usize, now: Instant) {
        if hidden > 0 {
            self.last = hidden;
        }
        self.shows.set_target(if hidden > 0 { 1.0 } else { 0.0 }, now);
    }
}

impl Strip {
    pub fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        let tray = Live::follow(TOPIC, cx, |Event::Tray(tray)| Some(tray));
        let follow_tray = cx.observe(&tray, |strip, _, cx| strip.read(cx));
        let follow_panels = Stage::observe_panels(cx, |strip, cx| strip.follow_open(cx));
        let mut strip = Self {
            screen: screen.clone(),
            shown,
            tray,
            apps: Vec::new(),
            attention: false,
            open: false,
            toggled: now,
            morph: Eased::at(0.0, now),
            scroll: Eased::lasting(0.0, DURATION_FAST, now),
            wheel: Wheel::default(),
            dot: Eased::at(0.0, now),
            dots: HashMap::new(),
            counts: [Count::new(now), Count::new(now)],
            middles: Rc::default(),
            _follow: [Some(follow_tray), follow_panels],
        };
        strip.read(cx);
        strip
    }

    /// Takes novad's latest tray.
    fn read(&mut self, cx: &mut Context<Self>) {
        let all = self.tray.read(cx).get().map(model::entries).unwrap_or_default();
        self.attention = model::any_attention(&all);
        self.apps = model::shown(&all);
        // The stage and the right pill redraw when this shows or hides it.
        self.shown.set(!self.apps.is_empty(), cx);
        let now = Instant::now();
        for app in &self.apps {
            let dot = self.dots.entry(app.key.clone()).or_insert_with(|| Eased::at(0.0, now));
            dot.set_target(if app.attention { 1.0 } else { 0.0 }, now);
        }
        let keys: Vec<&String> = self.apps.iter().map(|app| &app.key).collect();
        self.dots.retain(|key, _| keys.contains(&key));
        if self.apps.is_empty() {
            // Never open with nothing to show.
            let screen = self.screen.id;
            cx.defer(move |cx| Stage::close(TRAY, screen, cx));
        }
        cx.notify();
    }

    /// The stage opened or closed the tray.
    fn follow_open(&mut self, cx: &mut Context<Self>) {
        let open = Stage::is_open(TRAY, self.screen.id, cx);
        if open == self.open {
            return;
        }
        let now = Instant::now();
        self.open = open;
        self.toggled = now;
        self.morph.set_target(if open { 1.0 } else { 0.0 }, now);
        if !open {
            let screen = self.screen.id;
            cx.defer(move |cx| Stage::close(menu::MENU, screen, cx));
        }
        cx.notify();
    }

    fn visible(&self) -> usize {
        self.apps.len().min(TRAY_VISIBLE)
    }

    fn strip_width(&self) -> Pixels {
        model::row_width(self.visible())
    }

    /// Scrolls whole icons, counting from where a scroll still running is
    /// going, so quick turns add up.
    fn scrolled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let notches = self.wheel.notches(event);
        if notches == 0 {
            return;
        }
        let target = model::scrolled(px(self.scroll.target()), notches, self.apps.len(), TRAY_VISIBLE);
        self.scroll.set_target(f32::from(target), Instant::now());
        cx.notify();
    }

    /// A click on `app`'s icon: left acts and closes the tray (an app with
    /// only a menu shows it), right shows its menu, middle is its
    /// secondary action.
    fn clicked(&mut self, app: &Entry, button: MouseButton, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        match button {
            MouseButton::Left if !app.opens_menu => {
                Daemon::send(Command::ActivateTrayItem { address: app.address.clone() }, cx);
                cx.defer(move |cx| Stage::close(TRAY, screen, cx));
            }
            MouseButton::Left | MouseButton::Right => {
                let middle = self.middles.borrow().get(&app.key).copied().unwrap_or_default();
                let menu = menu::of(&self.screen, cx);
                let app = app.clone();
                menu.update(cx, |menu, cx| menu.show(app, middle, cx));
            }
            MouseButton::Middle => {
                Daemon::send(Command::SecondaryActivateTrayItem { address: app.address.clone() }, cx);
            }
            _ => {}
        }
    }

    /// How far the icons have popped in, 0 to 1, as scale and opacity:
    /// opening, each a little after the one before; closing, all at once.
    fn pop(&self, index: usize, now: Instant) -> (f32, f32) {
        let since = now.saturating_duration_since(self.toggled);
        let wait = if self.open { TRAY_STAGGER * (index as u32 + 2) } else { Duration::ZERO };
        let moved = since.saturating_sub(wait);
        let grow = ease(fraction(moved, DURATION_PANEL));
        let fade = fraction(moved, DURATION_FADE);
        if self.open { (grow, fade) } else { (1.0 - grow, 1.0 - fade) }
    }

    /// How much the "+N"s show: they wait for the pill to have grown most
    /// of the way, then fade in; closing, they go with the pill.
    fn counts_reveal(&self, now: Instant) -> f32 {
        if !self.open {
            return self.morph.value(now);
        }
        let since = now.saturating_duration_since(self.toggled);
        fraction(since.saturating_sub(DURATION_PANEL.mul_f32(REVEAL_PILL_COUNT)), DURATION_FADE)
    }

    fn moving(&self, now: Instant) -> bool {
        let popping = TRAY_STAGGER * (self.visible() as u32 + 2) + DURATION_PANEL.max(DURATION_FADE);
        !self.morph.settled(now)
            || !self.scroll.settled(now)
            || !self.dot.settled(now)
            || self.dots.values().any(|dot| !dot.settled(now))
            || self.counts.iter().any(|count| !count.shows.settled(now))
            || now.saturating_duration_since(self.toggled) < popping
    }

    fn icon(&self, index: usize, app: &Entry, look: IconLook, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let dot = self.dots.get(&app.key).map_or(0.0, |dot| dot.value(look.now));
        let middles = self.middles.clone();
        let key = app.key.clone();
        let record = canvas(
            move |bounds, _, _| {
                middles.borrow_mut().insert(key.clone(), bounds.center().x);
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        // Drawn smaller to pop in: GPUI scales no element but a picture.
        let picture =
            AppIcon::new(app.key.clone(), app.name.clone()).icon(app.icon.clone()).size(TRAY_ICON_SIZE * look.scale);
        let (left, right, middle) = (app.clone(), app.clone(), app.clone());
        div()
            .id(SharedString::from(format!("tray-{}", app.key)))
            .absolute()
            .left(model::step() * index as f32 - look.scroll)
            .top_0()
            .size(PILL_BUTTON_SIZE)
            .rounded(PILL_BUTTON_SIZE / 2.0)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(theme.hover()))
            .opacity(look.opacity)
            .child(record)
            .child(picture)
            .children((dot > 0.0).then(|| attention_dot(dot, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |strip, _: &MouseDownEvent, _, cx| {
                    strip.clicked(&left, MouseButton::Left, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |strip, _: &MouseDownEvent, _, cx| {
                    strip.clicked(&right, MouseButton::Right, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |strip, _: &MouseDownEvent, _, cx| {
                    strip.clicked(&middle, MouseButton::Middle, cx);
                }),
            )
            .into_any_element()
    }
}

/// What one icon looks like this frame.
#[derive(Clone, Copy)]
struct IconLook {
    now: Instant,
    scroll: Pixels,
    scale: f32,
    opacity: f32,
}

fn fraction(elapsed: Duration, over: Duration) -> f32 {
    if over.is_zero() { 1.0 } else { (elapsed.as_secs_f32() / over.as_secs_f32()).min(1.0) }
}

/// The dot of an app asking for attention, on an icon's top right, ringed
/// in the background color; `grown` is its scale.
pub(crate) fn attention_dot(grown: f32, cx: &App) -> impl IntoElement {
    let theme = Theme::of(cx);
    let size = TRAY_DOT_SIZE * grown;
    let corner = TRAY_DOT_RING + (TRAY_DOT_SIZE - size) / 2.0;
    div()
        .absolute()
        .top(corner)
        .right(corner)
        .size(size)
        .rounded(size / 2.0)
        .bg(theme.primary)
        .border(TRAY_DOT_RING * grown)
        .border_color(theme.bg)
}

impl Strip {
    /// The icons, scrolling sideways, their ends fading out where more are
    /// out of sight. Nothing while the tray is shut.
    fn strip(&self, now: Instant, scroll: Pixels, strip_right: Pixels, cx: &mut Context<Self>) -> Stateful<Div> {
        let strip_width = self.strip_width();
        let morph = self.morph.value(now);
        let fades_left = scroll > px(0.0);
        let fades_right = scroll < model::row_width(self.apps.len()) - strip_width - px(1.0);
        let icons: Vec<AnyElement> = if morph > 0.0 {
            self.apps
                .iter()
                .enumerate()
                .map(|(index, app)| {
                    let (grow, fade) = self.pop(index, now);
                    let in_view = model::step() * index as f32 - scroll;
                    let edge = model::edge_fade(in_view, strip_width, fades_left, fades_right);
                    let scale = ENTER_SCALE + (1.0 - ENTER_SCALE) * grow;
                    self.icon(index, app, IconLook { now, scroll, scale, opacity: fade * edge }, cx)
                })
                .collect()
        } else {
            Vec::new()
        };
        div()
            .id("strip")
            .absolute()
            .top(button_top())
            .h(PILL_BUTTON_SIZE)
            .right(strip_right)
            .w(strip_width)
            .overflow_hidden()
            .opacity(morph)
            .children(icons)
            .on_scroll(cx.listener(|strip, event: &ScrollEvent, _, cx| strip.scrolled(event, cx)))
    }

    /// The tray's chevron, pointing at where it opens; it turns as it does.
    fn chevron(&self, morph: f32, now: Instant, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::of(cx);
        let (fg, hover) = (theme.fg, theme.hover());
        let screen = self.screen.id;
        let dot = self.dot.value(now);
        div()
            .id("chevron")
            .absolute()
            .top(button_top())
            .right_0()
            .size(PILL_BUTTON_SIZE)
            .rounded(PILL_BUTTON_SIZE / 2.0)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(Icon::new(Glyph::LEFT).color(fg).turned(morph * 180.0))
            .children((dot > 0.0).then(|| attention_dot(dot, cx)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| Stage::toggle(TRAY, screen, cx))
    }
}

impl Render for Strip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let fg_dim = Theme::of(cx).fg_dim();
        let total = self.apps.len();
        let strip_width = self.strip_width();
        // Hidden apps or apps gone: keep the scroll inside the row.
        let furthest = f32::from((model::row_width(total) - strip_width).max(px(0.0)));
        if self.scroll.target() > furthest {
            self.scroll.set_target(furthest, now);
        }
        let scroll = px(self.scroll.value(now));
        let out = model::out_of_sight(total, TRAY_VISIBLE, scroll / model::step());
        self.counts[0].set(out.left, now);
        self.counts[1].set(out.right, now);
        self.dot.set_target(if self.attention && !self.open { 1.0 } else { 0.0 }, now);
        let morph = self.morph.value(now);
        if self.moving(now) {
            window.request_animation_frame();
        }

        // With more than fit, a "+N" on each side keeps its room.
        let has_more = total > TRAY_VISIBLE;
        let count_width = if has_more { count_width(total - TRAY_VISIBLE, window) } else { px(0.0) };
        let more = if has_more { count_width + PILL_BUTTON_GAP } else { px(0.0) };
        let open_width = strip_width + more * 2.0 + PILL_BUTTON_GAP;
        let strip_right = PILL_BUTTON_SIZE + PILL_BUTTON_GAP + more;

        let strip = self.strip(now, scroll, strip_right, cx);

        let reveal = self.counts_reveal(now);
        let counts = (has_more && morph > 0.0).then(|| {
            let counts =
                [(&self.counts[0], strip_right + strip_width + PILL_BUTTON_GAP), (&self.counts[1], strip_right - more)];
            counts.map(|(count, right)| {
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .right(right)
                    .w(count_width)
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(FONT_MONO)
                    .text_size(FONT_SIZE_MICRO)
                    .text_color(fg_dim)
                    .opacity(reveal * count.shows.value(now))
                    .child(format!("+{}", count.last))
            })
        });

        let chevron = self.chevron(morph, now, cx);

        div()
            .relative()
            .h(BAR_HEIGHT)
            .w(open_width * morph + PILL_BUTTON_SIZE)
            .flex_none()
            .when(!self.morph.settled(now), |row| row.overflow_hidden())
            .child(strip)
            .children(counts.into_iter().flatten())
            .child(chevron)
    }
}

/// How wide a "+N" is: as wide as the most there can be on a side, so it
/// keeps its room as the number changes. The face's digits are all as wide.
fn count_width(most: usize, window: &Window) -> Pixels {
    let text = window.text_system();
    let id = text.resolve_font(&font(FONT_MONO));
    let advance = text.ch_advance(id, FONT_SIZE_MICRO).unwrap_or(FONT_SIZE_MICRO);
    advance * format!("+{most}").chars().count() as f32
}
