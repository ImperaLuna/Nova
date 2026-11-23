//! What the AI usage feature draws, with GPUI: the `ai-usage.pill` view of
//! `features/ai-usage/feature.toml` (host `bar.left`) and the `ai-usage`
//! panel it grows into. `ui/ai_usage` in today's shell, which stays the
//! spec until it is removed.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `pill`: the shown tool's logo and its session percent, once novad has
//!   numbers.
//! - `panel`: the shown tool's limits and when they reset, and the other
//!   tools, a click on one showing it in the bar.
//! - `usage`: what each shows, worked out from novad's event, tested alone;
//!   `layout`: how tall the panel is for it.
//! - `logo`: the tools' marks. `tokens`: the feature's own values.

mod layout;
mod logo;
mod panel;
mod pill;
mod tokens;
mod usage;

use gpui::{App, AppContext};
use nova_stage::{Panel, Pill};

/// The view id the manifest's slot names.
const PILL_VIEW: &str = "ai-usage.pill";

/// Registers the pill, with its panel, with the stage.
pub fn register(cx: &mut App) {
    nova_stage::pill(PILL_VIEW, cx, |screen, cx| {
        let content = cx.new(|cx| panel::UsagePanel::new(screen, cx));
        let size = content.read(cx).size();
        let pill = cx.new(|cx| pill::UsagePill::new(screen, cx));
        let shown = pill.read(cx).shown();
        Pill::new(pill).shown_while(shown).panel(Panel::new(panel::NAME, size, content))
    });
}
