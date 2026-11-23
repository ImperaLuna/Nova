//! What the screenshot feature draws with GPUI, as its manifest's slot
//! names the view (`features/screenshot/feature.toml`): `capture.overlay`
//! (host `shell.overlays`), the region picker and the card a saved shot
//! lands in. `ui/capture` in today's shell, which stays the spec until it
//! is removed.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `capture`: the link to novad and what every screen shares, and the
//!   windows on each screen.
//! - `picker` and `toolbar`: one screen's region picker.
//! - `card`: the card a saved shot lands in.
//! - `pick`: the picker's and the card's sums, tested alone; `images`
//!   reads the pictures; `tokens`: the feature's own sizes.

mod capture;
mod card;
mod images;
mod pick;
mod picker;
mod tokens;
mod toolbar;

use gpui::App;

/// Registers the overlay with the stage, which starts it while screenshots
/// are on.
pub fn register(cx: &mut App) {
    nova_stage::overlay("capture.overlay", cx, |cx| Box::new(capture::Capture::start(cx)));
}
