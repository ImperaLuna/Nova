//! The notifications' logic without drawing: the list's groups, how long
//! a pop-up stays, how long ago one came, and a low battery alert's fill.
//! `ui/notifications/notifications.js` and parts of `ui/text/duration.js`
//! and `ui/quick_settings/devices.js` in today's shell.

use nova_notifications_api::{Notification, Urgency};
use nova_peripherals_api::{Battery, BatteryLevel};
use std::time::Duration;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const PERCENT: u8 = 100;

/// One app's notifications in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Group<'a> {
    pub app: &'a str,
    /// Newest first.
    pub items: Vec<&'a Notification>,
}

/// `items` (newest first) grouped by app, the group with the newest first.
pub fn groups(items: &[Notification]) -> Vec<Group<'_>> {
    let mut found: Vec<Group<'_>> = Vec::new();
    for item in items {
        match found.iter_mut().find(|group| group.app == item.app) {
            Some(group) => group.items.push(item),
            None => found.push(Group { app: &item.app, items: vec![item] }),
        }
    }
    found
}

pub fn unread(items: &[Notification]) -> usize {
    items.iter().filter(|item| item.unread).count()
}

/// How long a pop-up of `notification` stays: what the app asked, else
/// `fallback`. Zero is until it's closed: critical ones, and progress still
/// running.
pub fn timeout(notification: &Notification, fallback: Duration) -> Duration {
    if notification.urgency == Urgency::Critical {
        return Duration::ZERO;
    }
    if notification.progress.is_some_and(|progress| progress < PERCENT) {
        return Duration::ZERO;
    }
    notification.timeout_ms.map_or(fallback, |ms| Duration::from_millis(ms.into()))
}

/// How long ago `time` was at `now` (both seconds since the epoch), in its
/// biggest whole unit: "now", "5m", "2h", "3d".
pub fn ago(time: i64, now: i64) -> String {
    let seconds = (now - time).max(0);
    if seconds < MINUTE {
        "now".into()
    } else if seconds < HOUR {
        format!("{}m", seconds / MINUTE)
    } else if seconds < DAY {
        format!("{}h", seconds / HOUR)
    } else {
        format!("{}d", seconds / DAY)
    }
}

/// How full a battery is drawn, 0 to 1.
pub fn battery_fill(battery: Battery) -> f32 {
    match battery {
        Battery::Percent(percent) => f32::from(percent.min(PERCENT)) / f32::from(PERCENT),
        Battery::Level(level) => match level {
            BatteryLevel::Critical => 0.05,
            BatteryLevel::Low => 0.25,
            BatteryLevel::Normal => 0.5,
            BatteryLevel::High => 0.75,
            BatteryLevel::Full => 1.0,
        },
    }
}

/// The letter an app without an icon shows: its name's first letter or digit.
pub fn letter(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map_or_else(|| "?".into(), |c| c.to_uppercase().collect())
}

/// Which of the theme's swatches (0 to `count` - 1) an app's letter sits
/// on: the same for the same id every time, as today's launcher picks it.
pub fn swatch(id: &str, count: usize) -> usize {
    let mut hash: i32 = 0;
    for unit in id.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    hash.unsigned_abs() as usize % count.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(id: u32, app: &str) -> Notification {
        Notification {
            id,
            app: app.into(),
            icon: None,
            app_id: None,
            summary: String::new(),
            body: String::new(),
            image: None,
            actions: vec![],
            has_default: false,
            urgency: Urgency::Normal,
            progress: None,
            timeout_ms: None,
            time: 0,
            unread: id.is_multiple_of(2),
            live: true,
            device: None,
        }
    }

    #[test]
    fn the_list_groups_by_app_with_the_newest_first() {
        let items = [from(4, "Discord"), from(3, "Firefox"), from(2, "Discord"), from(1, "Mail")];
        let found = groups(&items);
        let shape: Vec<(&str, Vec<u32>)> =
            found.iter().map(|group| (group.app, group.items.iter().map(|item| item.id).collect())).collect();
        assert_eq!(shape, [("Discord", vec![4, 2]), ("Firefox", vec![3]), ("Mail", vec![1])]);
        assert_eq!(unread(&items), 2);
    }

    #[test]
    fn how_long_ago_is_its_biggest_whole_unit() {
        assert_eq!(ago(100, 130), "now");
        assert_eq!(ago(0, 5 * 60 + 59), "5m");
        assert_eq!(ago(0, 2 * 3600 + 10), "2h");
        assert_eq!(ago(0, 3 * 86400), "3d");
        assert_eq!(ago(100, 50), "now", "a clock that went back");
    }

    #[test]
    fn an_app_without_an_icon_gets_its_letter_on_the_launcher_s_swatch() {
        assert_eq!(letter("firefox"), "F");
        assert_eq!(letter("  7zip"), "7");
        assert_eq!(letter("--"), "?");
        // launcher.js's swatch(), counted from 0: "firefox" is swatch 8 there.
        assert_eq!(swatch("firefox", 8), 7);
        assert_eq!(swatch("", 8), 0);
    }

    #[test]
    fn a_battery_that_gives_only_a_step_is_drawn_at_that_step() {
        assert_eq!(battery_fill(Battery::Percent(40)), 0.4);
        assert_eq!(battery_fill(Battery::Percent(140)), 1.0);
        assert_eq!(battery_fill(Battery::Level(BatteryLevel::Critical)), 0.05);
    }
}
