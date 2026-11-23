//! What `quick_settings.js` and the drawer's QML worked out, with no GPUI
//! drawing in it: the greeting, the uptime, which logo, the button's badge,
//! when each section shows as the drawer opens, where a page sits as pages
//! swap, and the room the top row leaves for the pill's buttons.

use crate::tokens::{BADGE_MAX, DURATION_PAGE, PAGE_SLIDE, REVEAL_FIRST, REVEAL_RISE, REVEAL_STAGGER};
use gpui::{Pixels, px};
use nova_host_info::OperatingSystem;
use nova_look::motion::ease;
use nova_look::tokens::{BAR_MARGIN, DURATION_PANEL, GAP, PILL_BUTTON_GAP, PILL_BUTTON_INSET, PILL_BUTTON_SIZE};
use std::time::Duration;

/// The drawer's first page, as `Stage::page` gives it: none.
pub const FIRST_PAGE: Option<&str> = None;

/// "Good morning" until noon, "Good afternoon" until 6 pm, then "Good
/// evening" until 5 am.
pub fn greeting(hour: i8) -> &'static str {
    match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

const MINUTE: i64 = 60;
const HOUR_MINUTES: i64 = 60;
const DAY_MINUTES: i64 = 24 * HOUR_MINUTES;

/// "up 3h 12m" for `seconds` since boot, whole minutes rounded down
/// (`duration.js`'s `uptime`).
pub fn uptime(seconds: i64) -> String {
    let minutes = seconds.max(0) / MINUTE;
    let (days, hours, rest) = (minutes / DAY_MINUTES, minutes % DAY_MINUTES / HOUR_MINUTES, minutes % HOUR_MINUTES);
    let span = match (days, hours, rest) {
        (1.., 0, _) => format!("{days}d"),
        (1.., _, _) => format!("{days}d {hours}h"),
        (_, 1.., 0) => format!("{hours}h"),
        (_, 1.., _) => format!("{hours}h {rest}m"),
        _ => format!("{rest}m"),
    };
    format!("up {span}")
}

/// Distributions with a logo of Nova's own (`ui/quick_settings/logos`).
const BUNDLED_LOGOS: [&str; 1] = ["nixos"];

/// Where the header's logo comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Logo {
    /// One of Nova's own, by distribution id.
    Bundled(&'static str),
    /// The icon theme's file for os-release's `LOGO`, which novad found.
    File(String),
    /// Nova's own mark.
    Mark,
}

/// The logo for the distribution `os`: Nova's own for it or one it derives
/// from (`ID_LIKE`), else the icon theme's, else Nova's mark.
pub fn logo(os: Option<&OperatingSystem>) -> Logo {
    let Some(os) = os else { return Logo::Mark };
    let ids = std::iter::once(&os.id).chain(&os.id_like);
    for id in ids {
        if let Some(bundled) = BUNDLED_LOGOS.iter().find(|bundled| **bundled == id) {
            return Logo::Bundled(bundled);
        }
    }
    os.logo.clone().map_or(Logo::Mark, Logo::File)
}

/// The greeting's size: its own where `wide` (the greeting and name at
/// `size`) fits `width`, else smaller, down to `least`. The name is cut
/// short after that.
pub fn greeting_size(size: Pixels, least: Pixels, wide: Pixels, width: Pixels) -> Pixels {
    if wide <= width || wide <= px(0.0) {
        return size;
    }
    (size * (width / wide)).floor().max(least)
}

/// What the button's badge shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Badge {
    /// How many are unread: "3", or "9+" past `BADGE_MAX`.
    Count(String),
    /// Do not disturb: a grey crossed-out bell.
    Quiet,
}

/// The badge while quick settings is `open` or not: none while open (the
/// list shows them), do not disturb before a count, a count only when
/// something is unread.
pub fn badge(unread: usize, do_not_disturb: bool, open: bool) -> Option<Badge> {
    match (open, do_not_disturb, unread) {
        (true, _, _) => None,
        (false, true, _) => Some(Badge::Quiet),
        (false, false, 0) => None,
        (false, false, count) if count > BADGE_MAX => Some(Badge::Count(format!("{BADGE_MAX}+"))),
        (false, false, count) => Some(Badge::Count(count.to_string())),
    }
}

/// How far the `order`th section of the first page has come in (0 to 1)
/// `since` the drawer started to open: it waits its turn, a share of the
/// morph, then eases in over a morph's time (`Reveal.qml`).
pub fn section_shown(order: usize, since: Duration) -> f32 {
    let delay = DURATION_PANEL.mul_f32(REVEAL_FIRST + REVEAL_STAGGER * order as f32);
    let along = since.saturating_sub(delay).as_secs_f32() / DURATION_PANEL.as_secs_f32();
    ease(along.clamp(0.0, 1.0))
}

/// How long the `count` sections take to be all in.
pub fn sections_take(count: usize) -> Duration {
    DURATION_PANEL.mul_f32(REVEAL_FIRST + REVEAL_STAGGER * count.saturating_sub(1) as f32) + DURATION_PANEL
}

/// How far below its place a section is while `shown` (0 to 1).
pub fn section_drop(shown: f32) -> Pixels {
    REVEAL_RISE * (1.0 - shown)
}

/// Where a page sits while it is `shown` (0 out, 1 in place), across from
/// its place: the first page off to the left, the others to the right.
pub fn page_offset(first: bool, shown: f32) -> Pixels {
    let away = if first { -PAGE_SLIDE } else { PAGE_SLIDE };
    away * (1.0 - shown)
}

/// How long pages take to swap.
pub const fn page_takes() -> Duration {
    DURATION_PAGE
}

/// The room the first page's top row leaves on its right for the pill's
/// `buttons`, which quick settings draws where the pill had them: from the
/// pages' right edge (`pad` in from the drawer's) to the pill's left end,
/// and a gap.
pub fn header_inset(buttons: usize, pad: Pixels) -> Pixels {
    if buttons == 0 {
        return px(0.0);
    }
    let count = buttons as f32;
    let row = PILL_BUTTON_SIZE * count + PILL_BUTTON_GAP * (count - 1.0);
    BAR_MARGIN + PILL_BUTTON_INSET * 2.0 + row - pad + GAP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greeting_follows_the_hour() {
        assert_eq!(greeting(4), "Good evening");
        assert_eq!(greeting(5), "Good morning");
        assert_eq!(greeting(11), "Good morning");
        assert_eq!(greeting(12), "Good afternoon");
        assert_eq!(greeting(17), "Good afternoon");
        assert_eq!(greeting(18), "Good evening");
        assert_eq!(greeting(0), "Good evening");
    }

    #[test]
    fn the_uptime_reads_as_duration_js_writes_it() {
        assert_eq!(uptime(59), "up 0m");
        assert_eq!(uptime(45 * 60 + 59), "up 45m");
        assert_eq!(uptime(2 * 3600), "up 2h");
        assert_eq!(uptime(2 * 3600 + 14 * 60), "up 2h 14m");
        assert_eq!(uptime(3 * 86_400), "up 3d");
        assert_eq!(uptime(3 * 86_400 + 2 * 3600 + 5 * 60), "up 3d 2h");
        assert_eq!(uptime(-5), "up 0m", "a clock that went back");
    }

    fn os(id: &str, like: &[&str], logo: Option<&str>) -> OperatingSystem {
        OperatingSystem {
            id: id.into(),
            id_like: like.iter().map(|id| (*id).into()).collect(),
            name: id.into(),
            logo: logo.map(Into::into),
        }
    }

    #[test]
    fn the_logo_is_nova_s_own_then_the_icon_theme_s_then_the_mark() {
        assert_eq!(logo(None), Logo::Mark);
        assert_eq!(logo(Some(&os("nixos", &[], Some("/x.svg")))), Logo::Bundled("nixos"));
        assert_eq!(logo(Some(&os("custom", &["nixos"], None))), Logo::Bundled("nixos"), "by what it derives from");
        assert_eq!(logo(Some(&os("arch", &[], Some("/arch.svg")))), Logo::File("/arch.svg".into()));
        assert_eq!(logo(Some(&os("linux", &[], None))), Logo::Mark);
    }

    #[test]
    fn a_long_greeting_shrinks_to_fit_but_no_further() {
        assert_eq!(greeting_size(px(24.0), px(15.0), px(200.0), px(250.0)), px(24.0));
        assert_eq!(greeting_size(px(24.0), px(15.0), px(300.0), px(250.0)), px(20.0));
        assert_eq!(greeting_size(px(24.0), px(15.0), px(900.0), px(250.0)), px(15.0));
    }

    #[test]
    fn the_badge_counts_unread_and_shows_do_not_disturb() {
        assert_eq!(badge(0, false, false), None);
        assert_eq!(badge(3, false, false), Some(Badge::Count("3".into())));
        assert_eq!(badge(9, false, false), Some(Badge::Count("9".into())));
        assert_eq!(badge(12, false, false), Some(Badge::Count("9+".into())));
        assert_eq!(badge(12, true, false), Some(Badge::Quiet));
        assert_eq!(badge(0, true, false), Some(Badge::Quiet));
        assert_eq!(badge(12, true, true), None, "none while open");
    }

    #[test]
    fn sections_come_in_one_after_another() {
        let at = |ms| Duration::from_millis(ms);
        assert_eq!(section_shown(0, at(0)), 0.0);
        assert_eq!(section_shown(0, at(80)), 0.0, "the first waits 35% of the morph");
        assert!(section_shown(0, at(200)) > section_shown(1, at(200)), "the next comes later");
        assert_eq!(section_shown(3, sections_take(4)), 1.0, "all in once they took their time");
        assert_eq!(section_drop(0.0), REVEAL_RISE);
        assert_eq!(section_drop(1.0), px(0.0));
    }

    #[test]
    fn pages_slide_in_from_their_side() {
        assert_eq!(page_offset(true, 0.0), -PAGE_SLIDE);
        assert_eq!(page_offset(false, 0.0), PAGE_SLIDE);
        assert_eq!(page_offset(false, 1.0), px(0.0));
    }

    #[test]
    fn the_top_row_leaves_room_for_the_pill_s_buttons() {
        assert_eq!(header_inset(0, px(16.0)), px(0.0));
        // QuickSettings.qml: output.width - pad - (buttonsX - inset) + gap,
        // with the pill's right end BAR_MARGIN from the screen's.
        assert_eq!(header_inset(1, px(16.0)), px(8.0 + 8.0 + 28.0 - 16.0 + 8.0));
        assert_eq!(header_inset(2, px(16.0)), px(8.0 + 8.0 + 58.0 - 16.0 + 8.0));
    }
}
