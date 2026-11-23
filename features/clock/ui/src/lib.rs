//! The clock's ui: the day and the time in the central pill, as the
//! `clock.time` part (`features/clock/feature.toml`). The bar's central
//! pill draws the pill around it and the divider before it; this draws
//! only the words.
//!
//! The time ticks on the minute, with no frame in between. When it changes,
//! each digit that changed rolls up and out while the new one rolls in from
//! below ([`digits`]); the others stand still. `NOVA_FROZEN_TIME` (an ISO
//! time like `2026-10-24T16:40`) shows a fixed time instead, for
//! screenshots that don't change with the clock.

mod digits;
mod time;

use gpui::{App, AppContext};

/// The view id the manifest's slot names.
const TIME_PART: &str = "clock.time";

pub fn register(cx: &mut App) {
    nova_stage::part(TIME_PART, cx, |_screen, cx| cx.new(time::ClockTime::start).into());
}
