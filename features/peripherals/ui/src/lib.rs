mod card;
mod logic;
mod tokens;

use gpui::{App, AppContext};
use nova_stage::{Part, Shown};

const CARD: &str = "peripherals.card";

pub fn register(cx: &mut App) {
    nova_stage::part(CARD, cx, |_, cx| {
        let shown = Shown::new(false);
        let card = cx.new(|cx| card::Card::new(shown.clone(), cx));
        Part::new(card).shown_while(shown)
    });
}
