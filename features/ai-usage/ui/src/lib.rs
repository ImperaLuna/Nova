mod layout;
mod logo;
mod panel;
mod pill;
mod tokens;
mod usage;

use gpui::{App, AppContext};
use nova_stage::{Panel, Pill};

const PILL_VIEW: &str = "ai-usage.pill";

pub fn register(cx: &mut App) {
    nova_stage::pill(PILL_VIEW, cx, |screen, cx| {
        let content = cx.new(|cx| panel::UsagePanel::new(screen, cx));
        let size = content.read(cx).size();
        let pill = cx.new(|cx| pill::UsagePill::new(screen, cx));
        let shown = pill.read(cx).shown();
        Pill::new(pill).shown_while(shown).panel(Panel::new(panel::NAME, size, content))
    });
}
