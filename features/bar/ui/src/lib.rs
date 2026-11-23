//! The bar's views on the GPUI stage: the two pill frames other features
//! fill with parts. The stage draws their glass and places them; these
//! views draw what is inside.
//!
//! - [`central_pill`]: the pill in the middle (view `central-pill`), with
//!   the search button that opens the launcher and the parts of
//!   `central-pill.parts` (the workspace numbers, the day and the time).
//! - [`right_pill`]: the pill at the right edge (view `right-pill`), with
//!   the buttons of `right-pill.buttons` (quick settings, power).

mod central_pill;
mod right_pill;

use gpui::App;

/// Registers the bar's views with the stage, one line per view.
pub fn register(cx: &mut App) {
    central_pill::register(cx);
    right_pill::register(cx);
}
