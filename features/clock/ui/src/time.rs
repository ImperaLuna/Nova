//! The `clock.time` view: the short day name and the date, dim, then the
//! 24-hour time, both in the mono face, as today's central pill writes them
//! (`ddd d`, `hh:mm`).

use crate::digits::{Digit, Digits};
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, ParentElement, Pixels, Render, SharedString, Styled, Window,
    div, font,
};
use jiff::Zoned;
use jiff::civil::DateTime;
use nova_look::Theme;
use nova_look::tokens::{FONT_MONO, FONT_SIZE, GAP, WEIGHT_LABEL, WEIGHT_TITLE};
use std::time::{Duration, Instant};

/// The day and the date, and the time: strftime's way of writing today's
/// `ddd d` and `hh:mm`.
const DAY_FORMAT: &str = "%a %-d";
const TIME_FORMAT: &str = "%H:%M";
/// A fixed time to show instead of the clock's, like `2026-10-24T16:40`.
const FROZEN_TIME_VAR: &str = "NOVA_FROZEN_TIME";
/// A wake-up this late into the next minute still counts as on time: the
/// timer never fires early, so the minute has always turned.
const ON_THE_MINUTE: Duration = Duration::from_millis(5);

pub struct ClockTime {
    now: DateTime,
    digits: Digits,
}

impl ClockTime {
    /// Shows the time now, then ticks on each minute until the view goes.
    /// A frozen time never ticks.
    pub fn start(cx: &mut Context<Self>) -> Self {
        let frozen = frozen_time();
        let now = frozen.unwrap_or_else(local_now);
        if frozen.is_none() {
            cx.spawn(async move |clock, cx| {
                let mut shown = true;
                while shown {
                    let wait = until_next_minute(local_now());
                    cx.background_executor().timer(wait).await;
                    shown = clock.update(cx, |clock, cx| clock.show(local_now(), cx)).is_ok();
                }
            })
            .detach();
        }
        Self { digits: Digits::new(&now.strftime(TIME_FORMAT).to_string(), Instant::now()), now }
    }

    fn show(&mut self, now: DateTime, cx: &mut Context<Self>) {
        let changed = now.strftime(TIME_FORMAT).to_string() != self.now.strftime(TIME_FORMAT).to_string()
            || now.date() != self.now.date();
        self.now = now;
        if changed {
            self.digits.set(&now.strftime(TIME_FORMAT).to_string(), Instant::now());
            cx.notify();
        }
    }
}

/// The time on the wall here, in the system's time zone.
fn local_now() -> DateTime {
    Zoned::now().datetime()
}

/// `NOVA_FROZEN_TIME`, when it is set to a time this can read.
fn frozen_time() -> Option<DateTime> {
    let text = std::env::var(FROZEN_TIME_VAR).ok()?;
    let time = text.parse::<DateTime>();
    if time.is_err() {
        eprintln!("WARN nova-ui: {FROZEN_TIME_VAR}={text} isn't a time like 2026-10-24T16:40, showing the clock's");
    }
    time.ok()
}

/// How long until the next minute starts, from `now`.
fn until_next_minute(now: DateTime) -> Duration {
    let seconds = u64::try_from(now.second()).unwrap_or(0);
    let nanos = u64::try_from(now.subsec_nanosecond()).unwrap_or(0);
    let into_minute = Duration::from_secs(seconds) + Duration::from_nanos(nanos);
    Duration::from_secs(60).saturating_sub(into_minute) + ON_THE_MINUTE
}

/// How one digit's cell is drawn: as wide as the face's digits (they are
/// all as wide in a mono face), and as tall as its glyphs.
#[derive(Clone, Copy)]
struct Cell {
    width: Pixels,
    height: Pixels,
}

impl Cell {
    fn of(window: &Window, weight: FontWeight) -> Self {
        let text = window.text_system();
        let mut face = font(FONT_MONO);
        face.weight = weight;
        let id = text.resolve_font(&face);
        let width = text.ch_advance(id, FONT_SIZE).unwrap_or(FONT_SIZE);
        let height = text.ascent(id, FONT_SIZE) + text.descent(id, FONT_SIZE).abs();
        Self { width, height }
    }
}

/// A character centered in a cell, `offset` down from its place.
fn glyph(character: char, cell: Cell, offset: Pixels, opacity: f32) -> Div {
    div()
        .absolute()
        .left_0()
        .top(offset)
        .w(cell.width)
        .h(cell.height)
        .flex()
        .items_center()
        .justify_center()
        .opacity(opacity)
        .child(SharedString::from(character.to_string()))
}

/// A digit that rolls: the old one up and out, the new one in from below,
/// clipped to its cell. The colon stands in a cell of its own width.
fn digit(digit: &Digit, cell: Cell, now: Instant) -> AnyElement {
    if !digit.rolls() {
        return div().child(SharedString::from(digit.shown.to_string())).into_any_element();
    }
    let roll = digit.roll.value(now);
    let mut slot = div().relative().w(cell.width).h(cell.height).overflow_hidden();
    if let Some(leaving) = digit.leaving.filter(|_| roll < 1.0) {
        slot = slot.child(glyph(leaving, cell, -cell.height * roll, 1.0 - roll));
    }
    slot.child(glyph(digit.shown, cell, cell.height * (1.0 - roll), roll)).into_any_element()
}

impl Render for ClockTime {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.digits.rolling(now) {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let cell = Cell::of(window, WEIGHT_TITLE);
        let day =
            div().font_weight(WEIGHT_LABEL).text_color(theme.fg_dim()).child(self.now.strftime(DAY_FORMAT).to_string());
        let time = div()
            .flex()
            .items_center()
            .font_weight(WEIGHT_TITLE)
            .text_color(theme.fg)
            .children(self.digits.iter().map(|each| digit(each, cell, now)));
        div().h_full().flex().items_center().gap(GAP).font_family(FONT_MONO).text_size(FONT_SIZE).child(day).child(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn at(hour: i8, minute: i8, second: i8, milli: i32) -> DateTime {
        date(2026, 10, 24).at(hour, minute, second, milli * 1_000_000)
    }

    #[test]
    fn it_wakes_just_after_the_minute_turns() {
        assert_eq!(until_next_minute(at(16, 39, 0, 0)), Duration::from_secs(60) + ON_THE_MINUTE);
        assert_eq!(until_next_minute(at(16, 39, 59, 500)), Duration::from_millis(500) + ON_THE_MINUTE);
    }

    #[test]
    fn it_writes_the_day_and_time_as_today_s_pill() {
        let time = at(9, 5, 0, 0);
        assert_eq!(time.strftime(DAY_FORMAT).to_string(), "Sat 24");
        assert_eq!(time.strftime(TIME_FORMAT).to_string(), "09:05");
    }

    #[test]
    fn a_frozen_time_reads_as_today_s_shell_writes_it() {
        let time: DateTime = "2026-10-24T16:40".parse().unwrap();
        assert_eq!(time, at(16, 40, 0, 0));
    }
}
