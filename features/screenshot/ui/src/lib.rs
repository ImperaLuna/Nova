//! What the screenshot and recording features draw with GPUI, as their
//! manifests' slots name the views (`features/screenshot/feature.toml`,
//! `features/recording/feature.toml`): `capture.overlay` (host
//! `shell.overlays`, shared by both), the region picker, the card a saved shot
//! or recording lands in and the marks around a region being recorded; and
//! `recording.pill` (host `bar.right`), shown while a recording runs.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `capture`: the link to novad and what every screen shares, and the
//!   windows on each screen; `capture::record` its recording side.
//! - `picker` and `toolbar`: one screen's region picker.
//! - `card`: the card a saved shot or recording lands in.
//! - `marks`: the marks around a recorded region.
//! - `pill`: the recording pill.
//! - `pick`: the picker's, the card's and the marks' sums, tested alone;
//!   `images` reads the pictures; `tokens`: the features' own sizes.

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

/// Registers the overlay and the recording pill with the stage, which
/// starts the overlay while screenshots or recordings are on, and builds
/// the pill while recordings are.
pub fn register(cx: &mut App) {
    nova_stage::overlay("capture.overlay", cx, |cx| Box::new(capture::Capture::start(cx)));
    nova_stage::pill("recording.pill", cx, |_screen, cx| {
        let pill = cx.new(pill::RecordingPill::new);
        let shown = pill.read(cx).shown();
        Pill::new(pill).shown_while(shown)
    });
}
