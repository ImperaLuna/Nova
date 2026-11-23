mod capture;
mod card;
mod images;
mod marks;
mod pick;
mod picker;
mod pill;
mod tokens;
mod toolbar;

use gpui::{App, AppContext};
use nova_stage::Pill;

pub fn register(cx: &mut App) {
    nova_stage::overlay("capture.overlay", cx, |cx| Box::new(capture::Capture::start(cx)));
    nova_stage::pill("recording.pill", cx, |_screen, cx| {
        let pill = cx.new(pill::RecordingPill::new);
        let shown = pill.read(cx).shown();
        Pill::new(pill).shown_while(shown)
    });
}
