mod logic;
mod page;
mod slider;

use gpui::{App, AppContext};

const SLIDER: &str = "sound.slider";
const PAGE: &str = "sound.page";
const QUICK_SETTINGS: &str = "quick-settings";

pub fn register(cx: &mut App) {
    nova_stage::part(SLIDER, cx, |screen, cx| cx.new(|cx| slider::SoundSlider::new(screen, cx)).into());
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::SoundPage::new(screen, cx)).into());
}
