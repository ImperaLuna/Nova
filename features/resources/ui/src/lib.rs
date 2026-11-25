mod card;
mod graph;
mod logic;
mod page;
mod ring;
mod tokens;

use gpui::{App, AppContext};

const CARD: &str = "resources.card";
const PAGE: &str = "resources.page";
const QUICK_SETTINGS: &str = "quick-settings";

pub fn register(cx: &mut App) {
    nova_stage::part(CARD, cx, |screen, cx| cx.new(|cx| card::Card::new(screen, cx)).into());
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::Page::new(screen, cx)).into());
}
