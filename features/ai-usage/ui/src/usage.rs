//! What the pill and the panel show for one provider of novad's `ai_usage`
//! event, with no drawing: `ui/ai_usage/usage.js` in today's shell. novad
//! does the work (sources, resets); this only turns its numbers into text.

use jiff::Timestamp;
use jiff::tz::TimeZone;
use nova_ai_usage_api::{AiProvider, AiUsage, ProviderUsage, UsageState, UsageWindow};

/// From this percent a limit turns the theme's primary color. One rule, no
/// further tiers.
pub const WARN_PERCENT: u8 = 80;

const MINUTE_SECONDS: i64 = 60;
const HOUR_MINUTES: i64 = 60;
const DAY_MINUTES: i64 = 24 * HOUR_MINUTES;
const DAY_SECONDS: i64 = DAY_MINUTES * MINUTE_SECONDS;

/// One limit, as the panel writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: &'static str,
    pub percent: u8,
    pub warn: bool,
    /// "2h 14m left", or "" once the window has reset.
    pub left: String,
    /// "Resets in 2h 14m", "Resets Mon 04:00", or "".
    pub reset: String,
}

/// Everything the pill and the panel need for one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub provider: Option<AiProvider>,
    pub name: &'static str,
    pub plan: String,
    pub state: UsageState,
    /// The session limit when there is one, else the only window.
    pub main: Option<Row>,
    pub second: Option<Row>,
    /// The bar's number, without its `%`: "42", or "…" while loading and
    /// "–" with no numbers.
    pub hero: String,
    /// Why there are no numbers, under the headline.
    pub message: String,
}

impl Summary {
    /// The hero's text with its unit: "42%", "…".
    pub fn hero_with_unit(&self) -> String {
        if self.main.is_some() { format!("{}%", self.hero) } else { self.hero.clone() }
    }

    pub fn warn(&self) -> bool {
        self.main.as_ref().is_some_and(|row| row.warn)
    }

    /// Not ready: the pill shows a ring beside the logo (spinning while
    /// loading, dashed when unavailable).
    pub fn has_indicator(&self) -> bool {
        self.state != UsageState::Ready
    }
}

pub fn is_warn(percent: u8) -> bool {
    percent >= WARN_PERCENT
}

pub fn provider_name(provider: AiProvider) -> &'static str {
    match provider {
        AiProvider::Claude => "Claude",
        AiProvider::Codex => "Codex",
    }
}

fn window_name(window: UsageWindow) -> &'static str {
    match window {
        UsageWindow::Session => "5-hour session",
        UsageWindow::Weekly => "Weekly",
    }
}

/// `minutes` as days and hours, or hours and minutes, leaving out a zero
/// second part: "3d 2h", "2h 14m", "45m" (`ui/text/duration.js`).
pub fn minutes(minutes: i64) -> String {
    let (days, hours, rest) = (minutes / DAY_MINUTES, minutes % DAY_MINUTES / HOUR_MINUTES, minutes % HOUR_MINUTES);
    match (days, hours, rest) {
        (0, 0, rest) => format!("{rest}m"),
        (0, hours, 0) => format!("{hours}h"),
        (0, hours, rest) => format!("{hours}h {rest}m"),
        (days, 0, _) => format!("{days}d"),
        (days, hours, _) => format!("{days}d {hours}h"),
    }
}

/// A span of `seconds`, rounded up to whole minutes, so it never reads 0m
/// before the reset.
fn duration(seconds: i64) -> String {
    minutes(((seconds + MINUTE_SECONDS - 1) / MINUTE_SECONDS).max(1))
}

/// Seconds from `now` until `resets_at`, when that is a time still ahead.
fn until(resets_at: Option<&str>, now: Timestamp) -> Option<(Timestamp, i64)> {
    let at: Timestamp = resets_at?.parse().ok()?;
    let seconds = at.as_second() - now.as_second();
    // Rounding up a part second still ahead keeps "1m" to the end.
    let seconds = if at > now { seconds.max(1) } else { 0 };
    (seconds > 0).then_some((at, seconds))
}

/// "2h 14m left", or "" once the window has reset.
pub fn left_text(resets_at: Option<&str>, now: Timestamp) -> String {
    until(resets_at, now).map(|(_, seconds)| format!("{} left", duration(seconds))).unwrap_or_default()
}

/// "Resets in 2h 14m" within a day, else "Resets Mon 04:00" in `zone`'s
/// time.
pub fn reset_text(resets_at: Option<&str>, now: Timestamp, zone: &TimeZone) -> String {
    let Some((at, seconds)) = until(resets_at, now) else { return String::new() };
    if seconds < DAY_SECONDS {
        return format!("Resets in {}", duration(seconds));
    }
    format!("Resets {}", at.to_zoned(zone.clone()).strftime("%a %H:%M"))
}

/// Everything the pill and the panel need for `provider` at `now`.
pub fn summarize(provider: Option<&ProviderUsage>, now: Timestamp, zone: &TimeZone) -> Summary {
    let state = provider.map_or(UsageState::Loading, |provider| provider.state);
    let limits = provider.filter(|_| state == UsageState::Ready).map(|provider| provider.limits.as_slice());
    let mut rows = limits.unwrap_or_default().iter().map(|limit| Row {
        name: window_name(limit.window),
        percent: limit.percent,
        warn: is_warn(limit.percent),
        left: left_text(limit.resets_at.as_deref(), now),
        reset: reset_text(limit.resets_at.as_deref(), now, zone),
    });
    let (main, second) = (rows.next(), rows.next());
    let hero = match (&main, state) {
        (Some(main), _) => main.percent.to_string(),
        (None, UsageState::Loading) => "…".to_owned(),
        (None, _) => "–".to_owned(),
    };
    let message = match (&main, state) {
        (Some(_), _) => String::new(),
        (None, UsageState::Loading) => "Checking your plan limits…".to_owned(),
        (None, _) => provider.and_then(|provider| provider.hint.clone()).unwrap_or_default(),
    };
    Summary {
        provider: provider.map(|provider| provider.provider),
        name: provider.map_or("", |provider| provider_name(provider.provider)),
        plan: provider.and_then(|provider| provider.plan.clone()).unwrap_or_default(),
        state,
        main,
        second,
        hero,
        message,
    }
}

/// The provider entry the bar shows: the one novad names, else the first.
pub fn shown(usage: &AiUsage) -> Option<&ProviderUsage> {
    usage.providers.iter().find(|provider| provider.provider == usage.shown).or(usage.providers.first())
}

/// The other providers, in novad's order.
pub fn others(usage: &AiUsage) -> impl Iterator<Item = &ProviderUsage> {
    usage.providers.iter().filter(|provider| provider.provider != usage.shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_ai_usage_api::UsageLimit;

    fn now() -> Timestamp {
        "2026-10-23T08:00:00Z".parse().unwrap()
    }

    fn limit(window: UsageWindow, percent: u8, resets_at: &str) -> UsageLimit {
        UsageLimit { window, percent, resets_at: Some(resets_at.to_owned()) }
    }

    /// The event `ui/tests/tst_ai_usage.qml` reads, in the shape novad sends.
    fn event() -> AiUsage {
        AiUsage {
            shown: AiProvider::Codex,
            providers: vec![
                ProviderUsage {
                    provider: AiProvider::Claude,
                    state: UsageState::Ready,
                    plan: None,
                    limits: vec![
                        limit(UsageWindow::Session, 42, "2026-10-23T10:14:00Z"),
                        limit(UsageWindow::Weekly, 61, "2026-10-27T03:00:00Z"),
                    ],
                    hint: None,
                },
                ProviderUsage {
                    provider: AiProvider::Codex,
                    state: UsageState::Ready,
                    plan: Some("Pro".to_owned()),
                    limits: vec![limit(UsageWindow::Weekly, 84, "2026-10-29T10:20:00Z")],
                    hint: None,
                },
            ],
        }
    }

    #[test]
    fn session_comes_first_and_weekly_second() {
        let usage = event();
        let claude = summarize(usage.providers.first(), now(), &TimeZone::UTC);
        assert_eq!(claude.name, "Claude");
        assert_eq!(claude.hero, "42");
        assert_eq!(claude.hero_with_unit(), "42%");
        let main = claude.main.as_ref().unwrap();
        assert_eq!((main.name, main.left.as_str()), ("5-hour session", "2h 14m left"));
        assert_eq!(claude.second.as_ref().unwrap().name, "Weekly");
        assert_eq!(claude.second.as_ref().unwrap().reset, "Resets Tue 03:00");
        assert_eq!(claude.plan, "");
        assert!(!claude.warn());
        assert!(!claude.has_indicator());
    }

    #[test]
    fn a_weekly_only_plan_shows_its_weekly_limit() {
        let usage = event();
        let codex = summarize(usage.providers.get(1), now(), &TimeZone::UTC);
        assert_eq!(codex.hero, "84");
        assert_eq!(codex.main.as_ref().unwrap().name, "Weekly");
        assert_eq!(codex.second, None);
        assert_eq!(codex.plan, "Pro");
        assert!(codex.warn(), "84% is past the warning line");
    }

    #[test]
    fn warning_starts_at_80() {
        assert!(!is_warn(79));
        assert!(is_warn(80));
    }

    #[test]
    fn reset_text_is_relative_within_a_day() {
        let utc = TimeZone::UTC;
        assert_eq!(reset_text(Some("2026-10-23T08:45:00Z"), now(), &utc), "Resets in 45m");
        assert_eq!(reset_text(Some("2026-10-23T11:00:00Z"), now(), &utc), "Resets in 3h");
        assert_eq!(reset_text(Some("2026-10-27T03:00:00Z"), now(), &utc), "Resets Tue 03:00");
        let east = TimeZone::fixed(jiff::tz::offset(1));
        assert_eq!(reset_text(Some("2026-10-27T03:00:00Z"), now(), &east), "Resets Tue 04:00", "in local time");
        assert_eq!(reset_text(Some("2026-10-23T07:00:00Z"), now(), &utc), "", "a past reset says nothing");
        assert_eq!(reset_text(None, now(), &utc), "");
        assert_eq!(reset_text(Some("not a time"), now(), &utc), "");
    }

    #[test]
    fn left_text_rounds_up_and_ends_at_the_reset() {
        assert_eq!(left_text(Some("2026-10-23T08:00:01Z"), now()), "1m left");
        assert_eq!(left_text(Some("2026-10-23T08:59:30Z"), now()), "1h left");
        assert_eq!(left_text(Some("2026-10-26T10:00:00Z"), now()), "3d 2h left");
        assert_eq!(left_text(Some("2026-10-23T08:00:00Z"), now()), "");
    }

    #[test]
    fn spans_read_as_duration_js_writes_them() {
        assert_eq!(minutes(45), "45m");
        assert_eq!(minutes(120), "2h");
        assert_eq!(minutes(134), "2h 14m");
        assert_eq!(minutes(3 * DAY_MINUTES), "3d");
        assert_eq!(minutes(3 * DAY_MINUTES + 2 * 60 + 5), "3d 2h");
    }

    #[test]
    fn loading_and_unavailable_have_no_numbers() {
        let loading = summarize(None, now(), &TimeZone::UTC);
        assert_eq!((loading.hero.as_str(), loading.hero_with_unit().as_str()), ("…", "…"));
        assert_eq!(loading.message, "Checking your plan limits…");
        assert!(loading.has_indicator());
        let unavailable = ProviderUsage {
            provider: AiProvider::Codex,
            state: UsageState::Unavailable,
            plan: None,
            limits: vec![limit(UsageWindow::Weekly, 10, "2026-10-29T10:20:00Z")],
            hint: Some("Sign in to Codex".to_owned()),
        };
        let summary = summarize(Some(&unavailable), now(), &TimeZone::UTC);
        assert_eq!((summary.hero.as_str(), summary.main.as_ref()), ("–", None));
        assert_eq!(summary.message, "Sign in to Codex");
        assert!(summary.has_indicator());
    }

    #[test]
    fn the_bar_shows_novad_s_pick_and_the_panel_lists_the_rest() {
        let mut usage = event();
        assert_eq!(shown(&usage).map(|provider| provider.provider), Some(AiProvider::Codex));
        let rest: Vec<_> = others(&usage).map(|provider| provider.provider).collect();
        assert_eq!(rest, [AiProvider::Claude]);
        usage.providers.remove(1);
        assert_eq!(shown(&usage).map(|provider| provider.provider), Some(AiProvider::Claude), "else the first");
    }
}
