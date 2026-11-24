mod central_pill;
mod right_pill;

use gpui::App;

pub fn register(cx: &mut App) {
    central_pill::register(cx);
    right_pill::register(cx);
}
