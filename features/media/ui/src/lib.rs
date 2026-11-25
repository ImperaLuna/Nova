mod art;
mod equalizer;
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

const PILL: &str = "media.pill";

pub fn register(cx: &mut App) {
    nova_stage::pill(PILL, cx, |screen, cx| {
        let model = cx.new(|cx| NowPlaying::new(screen, cx));
        let (size, shown) = (model.read(cx).panel_size, model.read(cx).shown.clone());
        let panel = cx.new(|cx| MediaPanel::new(model.clone(), cx));
        let pill = cx.new(|cx| MediaPill::new(model, cx));
        Pill::new(pill).panel(Panel::new(NAME, size, panel)).shown_while(shown)
    });
}
