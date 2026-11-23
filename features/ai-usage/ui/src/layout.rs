//! How tall the panel is for what it shows: `contentHeight` in
//! `ui/ai_usage/AiUsage.qml`, from the same tokens. No drawing; the one
//! thing only drawing knows, how many lines a hint wraps to, comes in.

use crate::usage::Summary;
use gpui::{Pixels, px};
use nova_look::tokens::{
    BAR_HEIGHT, FONT_SIZE, FONT_SIZE_SMALL, GAP, LINE_HEIGHT, LINE_HEIGHT_HERO, LINE_HEIGHT_WRAPPED, METER_HEIGHT,
    METER_HEIGHT_SMALL, PANEL_PAD_BOTTOM, ROW_HEIGHT, SECTION_GAP,
};

/// One line of small text, and of body text.
pub fn caption_line() -> Pixels {
    FONT_SIZE_SMALL * LINE_HEIGHT
}

pub fn body_line() -> Pixels {
    FONT_SIZE * LINE_HEIGHT
}

/// Things stacked `GAP` apart, as a QML Column lays them: empty ones take
/// no room and no gap.
fn stacked(heights: &[Pixels], gap: Pixels) -> Pixels {
    let shown: Vec<Pixels> = heights.iter().copied().filter(|height| *height > px(0.0)).collect();
    let gaps = gap * shown.len().saturating_sub(1) as f32;
    shown.into_iter().fold(gaps, |sum, height| sum + height)
}

/// The panel's height for `summary` with `others` other tools listed, the
/// hint wrapping to `message_lines`.
pub fn panel_height(summary: &Summary, others: usize, message_lines: usize) -> Pixels {
    let caption = caption_line();
    let main = summary.main.as_ref();
    let lone_reset = main.filter(|main| summary.second.is_none() && !main.reset.is_empty());
    let message = if summary.message.is_empty() {
        px(0.0)
    } else {
        FONT_SIZE_SMALL * LINE_HEIGHT_WRAPPED * message_lines.max(1) as f32
    };
    let primary = stacked(
        &[
            main.map_or(px(0.0), |_| caption),
            main.map_or(px(0.0), |_| METER_HEIGHT),
            lone_reset.map_or(px(0.0), |_| caption),
            message,
        ],
        GAP,
    );
    let second = summary.second.as_ref().map_or(px(0.0), |second| {
        let reset = if second.reset.is_empty() { px(0.0) } else { caption };
        stacked(&[body_line(), METER_HEIGHT_SMALL, reset], GAP)
    });
    let details = stacked(&[primary, second], SECTION_GAP);
    // The line over the other tools, and a row for each.
    let others = px(1.0) + (GAP + ROW_HEIGHT) * others as f32;
    BAR_HEIGHT + GAP + LINE_HEIGHT_HERO + details + SECTION_GAP + others + PANEL_PAD_BOTTOM
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Row;
    use nova_ai_usage_api::UsageState;

    fn row(reset: &str) -> Row {
        Row { name: "Weekly", percent: 10, warn: false, left: String::new(), reset: reset.to_owned() }
    }

    fn summary(main: Option<Row>, second: Option<Row>, message: &str) -> Summary {
        Summary {
            provider: None,
            name: "Claude",
            plan: String::new(),
            state: UsageState::Ready,
            main,
            second,
            hero: String::new(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn two_limits_stack_as_today_s_panel() {
        let both = summary(Some(row("")), Some(row("Resets Mon 04:00")), "");
        let primary = caption_line() + GAP + METER_HEIGHT;
        let second = body_line() + GAP + METER_HEIGHT_SMALL + GAP + caption_line();
        let others = px(1.0) + GAP + ROW_HEIGHT;
        let expected = BAR_HEIGHT
            + GAP
            + LINE_HEIGHT_HERO
            + primary
            + SECTION_GAP
            + second
            + SECTION_GAP
            + others
            + PANEL_PAD_BOTTOM;
        assert!((panel_height(&both, 1, 1) - expected).abs() < px(0.001));
    }

    #[test]
    fn a_lone_limit_says_when_it_resets_under_its_meter() {
        let lone = summary(Some(row("Resets in 2h")), None, "");
        let without = summary(Some(row("")), None, "");
        let grown = panel_height(&lone, 1, 1) - panel_height(&without, 1, 1);
        assert!((grown - (GAP + caption_line())).abs() < px(0.001), "{grown:?}");
    }

    #[test]
    fn a_hint_grows_by_its_lines() {
        let one = panel_height(&summary(None, None, "Sign in"), 1, 1);
        let three = panel_height(&summary(None, None, "Sign in"), 1, 3);
        assert!((three - one - FONT_SIZE_SMALL * LINE_HEIGHT_WRAPPED * 2.0).abs() < px(0.001));
        assert!(panel_height(&summary(None, None, ""), 1, 1) < one);
    }
}
