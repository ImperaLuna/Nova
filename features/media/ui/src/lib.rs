//! What the media feature draws, with GPUI: the now-playing pill in the
//! bar's left row (`bar.left` slot `media.pill` of
//! `features/media/feature.toml`) and the panel it grows into. `ui/media`
//! in today's shell, which stays the spec until it is removed.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `model`: one screen's shared state: novad's players, the pill being
//!   out, the panel open, the timers, and what it asks novad.
//! - `pill`, `panel`: the two views, which only draw.
//! - `art`: the app's icon and the cover, or a music note.
//! - `logic`: what `media.js` worked out (positions, times, the chips, the
//!   title's scroll and fade), tested alone.
//! - `measure`: text widths and lines, for sizes asked before drawing.
//! - `tokens`: the feature's own sizes.

mod art;
mod logic;
mod measure;
mod model;
mod panel;
mod pill;
mod tokens;

use gpui::{App, AppContext};
use model::{NAME, NowPlaying};
use nova_stage::{Panel, Pill};
use panel::MediaPanel;
use pill::MediaPill;

/// The view id the manifest's slot names.
const PILL: &str = "media.pill";

/// Registers the now-playing pill and its panel with the stage. The pill
/// takes room only while something plays or is paused.
pub fn register(cx: &mut App) {
    nova_stage::pill(PILL, cx, |screen, cx| {
        let model = cx.new(|cx| NowPlaying::new(screen, cx));
        let (size, shown) = (model.read(cx).panel_size, model.read(cx).shown.clone());
        let panel = cx.new(|cx| MediaPanel::new(model.clone(), cx));
        let pill = cx.new(|cx| MediaPill::new(model, cx));
        Pill::new(pill).panel(Panel::new(NAME, size, panel)).shown_while(shown)
    });
}
