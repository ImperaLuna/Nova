//! What the peripherals feature draws, with GPUI, in quick settings: the
//! Devices card (`quick-settings.cards` slot `peripherals.card` of
//! `features/peripherals/feature.toml`), shown while any wireless device is
//! connected.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `card`: the card.
//! - `logic`: what the card works out, tested alone.
//! - `tokens`: the feature's own sizes.

mod card;
mod logic;
mod tokens;

use gpui::{App, AppContext};
use nova_stage::{Part, Shown};

/// The view id the manifest's slot names.
const CARD: &str = "peripherals.card";

/// Registers the card with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(CARD, cx, |_, cx| {
        let shown = Shown::new(false);
        let card = cx.new(|cx| card::Card::new(shown.clone(), cx));
        Part::new(card).shown_while(shown)
    });
}
