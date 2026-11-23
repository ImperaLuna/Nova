mod card;
mod carousel;
mod deck;
mod draw;
mod images;
mod others;
mod preview;
mod tilt;
mod tokens;

use carousel::{Carousel, NAME};
use gpui::{App, AppContext};
use nova_stage::{Panel, Placement};

pub fn register(cx: &mut App) {
    nova_stage::panel("wallpaper.carousel", cx, |screen, cx| {
        let carousel = cx.new(|cx| Carousel::new(screen, cx));
        let focus = carousel.read(cx).focus.clone();
        Panel::new(NAME, screen.size, carousel).placement(Placement::WholeScreen).takes_typing(focus)
    });
}
