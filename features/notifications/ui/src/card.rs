//! A pop-up's card: who it's from, the app, the summary and body (a low battery
//! alert's battery and percent instead), progress, and the app's buttons (the
//! first in the accent). A thin line along the bottom drains over its time; the
//! pointer on it holds it, and shows ✕. Critical ones have a rim in the error
//! color. It grows out of the quick settings button over its top right corner,
//! folds back into it, or slides aside.

use crate::logic::ago;
use crate::pieces::{battery_amount, meter, who};
use crate::popups::{Act, Reach};
use crate::stack::{Entry, Leaving};
use crate::tokens::{
    ACTION_HEIGHT, ACTIONS_GAP, APP_LINE, AVATAR, AWAY, BODY_LINES, CLOSE_INSET, CLOSE_SIZE, CRITICAL_RIM, FOLD,
    FOLD_HEIGHT, LINE_GAP, PAD_BOTTOM, PROGRESS_HEIGHT, RADIUS, RISE, TIMER_HEIGHT, TIMER_OPACITY, WIDTH,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Bounds, Div, InteractiveElement, IntoElement, ParentElement, Pixels, StatefulInteractiveElement,
    Styled, div, point, px, relative, size,
};
use nova_look::tokens::{
    FONT_MONO, FONT_SIZE, FONT_SIZE_MICRO, FONT_SIZE_SMALL, GAP, ICON_GAP, LABEL_NUDGE, PAD, SHADOW_ROOM, WEIGHT_LABEL,
    WEIGHT_TITLE,
};
use nova_look::{Glass, Glyph, RoundButton, Theme};
use nova_notifications_api::{Notification, Urgency};
use std::time::Instant;

/// A share of a whole, as progress counts it.
const PERCENT: f32 = 100.0;

/// One card as this frame draws it.
pub struct Look<'a> {
    pub entry: &'a Entry,
    /// 0 gone to 1 in place.
    pub presence: f32,
    /// Its height once in place, and the top of its slot.
    pub height: Pixels,
    pub slot_top: Pixels,
    pub now: Instant,
    /// Seconds since the epoch, for how long ago it came.
    pub wall: i64,
}

impl Look<'_> {
    /// Where its glass is: folded toward the button and raised, or slid
    /// aside, by how present it is; and its corners' radius.
    pub fn bounds(&self) -> (Bounds<Pixels>, Pixels) {
        let presence = self.presence;
        let away = self.entry.leaving == Some(Leaving::Away);
        let x_scale = if away { 1.0 } else { FOLD + (1.0 - FOLD) * presence };
        let y_scale = if away { 1.0 } else { FOLD_HEIGHT + (1.0 - FOLD_HEIGHT) * presence };
        let shift_x = if away { AWAY * (1.0 - presence) } else { px(0.0) };
        let shift_y = if away { px(0.0) } else { -(RISE * (1.0 - presence)) };
        let bounds = Bounds {
            origin: point(SHADOW_ROOM + WIDTH * (1.0 - x_scale) + shift_x, self.slot_top + shift_y),
            size: size(WIDTH * x_scale, self.height * y_scale),
        };
        (bounds, RADIUS * x_scale.min(y_scale))
    }
}

/// The card, its content laid out at full size inside the glass, and
/// `measure` (which records the content's height) laid over it.
pub fn card(look: &Look<'_>, reach: &Reach, measure: impl IntoElement, cx: &App) -> AnyElement {
    let (bounds, radius) = look.bounds();
    let theme = Theme::of(cx);
    let (entry, presence) = (look.entry, look.presence);
    let key = entry.key;
    let (hover, open) = (reach.clone(), reach.clone());
    let content =
        div().absolute().top_0().right_0().w(WIDTH).opacity(presence).child(content(look, reach, cx)).child(measure);
    div()
        .id(("notification", key))
        .absolute()
        .left(bounds.origin.x)
        .top(bounds.origin.y)
        .w(bounds.size.width)
        .h(bounds.size.height)
        .cursor_pointer()
        .on_hover(move |hovered, _, cx| hover.hover(key, *hovered, cx))
        .on_click(move |_, _, cx| open.act(key, Act::Open, cx))
        .child(Glass::new(radius).shadow(presence).build(cx).absolute().inset_0().opacity(presence))
        .when(entry.held, |card| card.child(div().absolute().inset_0().rounded(radius).bg(theme.hover())))
        .when(entry.notification.urgency == Urgency::Critical, |card| {
            card.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(radius)
                    .border(CRITICAL_RIM)
                    .border_color(theme.error)
                    .opacity(presence),
            )
        })
        .child(div().absolute().inset_0().overflow_hidden().child(content))
        .into_any_element()
}

/// What the card shows, at its full size.
fn content(look: &Look<'_>, reach: &Reach, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let entry = look.entry;
    let key = entry.key;
    let inset = PAD + LABEL_NUDGE * 2.0;
    let dismiss = entry.held.then(|| {
        let reach = reach.clone();
        div().absolute().top(CLOSE_INSET).right(CLOSE_INSET).child(
            RoundButton::new(("dismiss", key), Glyph::CLOSE, CLOSE_SIZE)
                .build(cx, move |_, _, cx| reach.act(key, Act::Dismiss, cx)),
        )
    });
    let remaining = entry.countdown.remaining(look.now);
    let timer = (entry.countdown.shown() && remaining > 0.0).then(|| {
        div()
            .absolute()
            .bottom_0()
            .left(inset)
            .right(inset)
            .h(TIMER_HEIGHT)
            .child(div().h_full().w(relative(remaining)).rounded_full().bg(theme.fg.alpha(TIMER_OPACITY)))
    });
    div()
        .relative()
        .w_full()
        .pt(PAD)
        .pb(PAD_BOTTOM)
        .px(inset)
        .flex()
        .gap(PAD)
        .child(div().flex_none().child(who(&entry.notification, AVATAR, cx)))
        .child(lines(entry, look.wall, reach, cx))
        .children(dismiss)
        .children(timer)
}

/// The app and how long ago, the summary, the body, progress and buttons.
fn lines(entry: &Entry, wall: i64, reach: &Reach, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let notification = &entry.notification;
    let battery = notification.device.map(|device| device.battery);
    let app_line = div()
        .h(APP_LINE)
        .flex()
        .items_center()
        .gap(GAP)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(FONT_SIZE_SMALL)
                .text_color(theme.fg_soft())
                .child(notification.app.clone()),
        )
        .when(!entry.held, |line| {
            line.child(
                div()
                    .font_family(FONT_MONO)
                    .text_size(FONT_SIZE_MICRO)
                    .text_color(theme.fg_soft())
                    .child(ago(notification.time, wall)),
            )
        });
    let summary = div()
        .flex()
        .items_center()
        .gap(GAP)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(FONT_SIZE)
                .font_weight(WEIGHT_TITLE)
                .child(notification.summary.clone()),
        )
        .children(battery.map(|battery| battery_amount(notification, battery, cx)));
    let body = (battery.is_none() && !notification.body.is_empty()).then(|| {
        div()
            .w_full()
            .text_size(FONT_SIZE_SMALL)
            .text_color(theme.fg_dim())
            .line_clamp(BODY_LINES)
            .text_ellipsis()
            .child(notification.body.clone())
    });
    let progress = notification.progress.map(|progress| meter(f32::from(progress) / PERCENT, PROGRESS_HEIGHT, cx));
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(LINE_GAP)
        .child(app_line)
        .child(summary)
        .children(body)
        .children(progress)
        .children(actions(notification, entry.key, reach, cx))
}

/// Its buttons, the first in the accent; those that don't fit on a line
/// wrap to the next.
fn actions(notification: &Notification, key: u64, reach: &Reach, cx: &App) -> Option<Div> {
    if notification.actions.is_empty() {
        return None;
    }
    let theme = Theme::of(cx);
    let buttons = notification.actions.iter().enumerate().map(|(at, action)| {
        let (reach, name) = (reach.clone(), action.key.clone());
        let button = div()
            .id(("action", at))
            .h(ACTION_HEIGHT)
            .max_w_full()
            .px(PAD)
            .flex()
            .items_center()
            .rounded_full()
            .text_size(FONT_SIZE_SMALL)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                reach.act(key, Act::Button(name.clone()), cx);
            })
            .child(div().min_w_0().truncate().child(action.label.clone()));
        if at == 0 {
            button.bg(theme.primary).text_color(theme.on_accent).font_weight(WEIGHT_TITLE)
        } else {
            let quiet = theme.selected_quiet();
            button.bg(theme.selected()).font_weight(WEIGHT_LABEL).hover(move |style| style.bg(quiet))
        }
    });
    Some(div().pt(ACTIONS_GAP).flex().flex_wrap().gap(ICON_GAP).children(buttons))
}
