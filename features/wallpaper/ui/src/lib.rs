//! What the wallpaper feature draws, with GPUI: the stage's `wallpaper`
//! panel (`stage.panels` slot `wallpaper.carousel` of
//! `features/wallpaper/feature.toml`), the carousel over the whole screen
//! that picks an image from the wallpaper folder. `ui/wallpaper` in
//! today's shell, which stays the spec until it is removed.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `carousel`: the panel's view, one per screen: its state, its keys,
//!   what it asks novad.
//! - `draw`: how it looks.
//! - `deck`: which image is picked, where each card sits and what Enter
//!   keeps, tested alone.
//! - `images`: the folder's images decoded at the size they are drawn.
//! - `preview`: a picture over the screen that fades to the next.
//! - `others`: the preview on the screens it isn't open on.
//! - `tokens`: the carousel's own sizes.

mod carousel;
mod deck;
mod draw;
mod images;
mod others;
mod preview;
mod tokens;

use carousel::{Carousel, NAME};
use gpui::{App, AppContext};
use nova_stage::Panel;

/// Registers the carousel with the stage: over the whole screen, holding
/// the keyboard while it is open.
pub fn register(cx: &mut App) {
    nova_stage::panel("wallpaper.carousel", cx, |screen, cx| {
        let carousel = cx.new(|cx| Carousel::new(screen, cx));
        let focus = carousel.read(cx).focus.clone();
        Panel::new(NAME, screen.size, carousel).covers_screen().takes_typing(focus)
    });
}
