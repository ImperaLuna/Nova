mod digits;
mod time;

use gpui::{App, AppContext};

const TIME_PART: &str = "clock.time";

pub fn register(cx: &mut App) {
    nova_stage::part(TIME_PART, cx, |_screen, cx| cx.new(time::ClockTime::start).into());
}
