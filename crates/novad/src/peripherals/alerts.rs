//! Low battery alerts: a notification when a device's battery drops to
//! [`STEPS`] (20%, 10%, then 5%, critical), each once per discharge.
//!
//! A device read for the first time already low gets one alert, for the
//! lowest step it's under. Charging says them all again next time; so does
//! a reading [`REARM`] points over a step (a device that comes back
//! charged), for that step and the ones under it, so a reading wobbling
//! around a step doesn't say it twice. A device that only reports a level
//! reads as [`level_percent`]: `low` is the 10% alert, `critical` the 5%
//! one. What's been said is kept by device ([`key`]) while novad runs, also
//! while a device is away, so one that reconnects still low isn't alerted
//! again.

use crate::notifications::Own;
use nova_protocol::{Battery, BatteryLevel, NotificationDevice, Peripheral, PeripheralKind};
use std::collections::HashMap;

/// The percents alerted at, highest first; the last one is critical.
pub const STEPS: [u8; 3] = [20, 10, 5];
/// How far over a step a reading has to go to say it again.
pub const REARM: u8 = 10;
/// Devices remembered; past it, ones not connected now are forgotten.
const MAX_REMEMBERED: usize = 64;
/// Who the alerts read as from.
pub const APP: &str = "Devices";

/// What's been said about each device.
#[derive(Debug, Default)]
pub struct Alerts {
    devices: HashMap<String, Said>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Said {
    /// The lowest step alerted this discharge.
    step: Option<u8>,
    /// The device's last alert, which its next one takes the place of.
    notification: u32,
}

/// An alert to send for the device `key` names.
#[derive(Debug, PartialEq)]
pub struct Alert {
    pub key: String,
    pub own: Own,
}

impl Alerts {
    /// The alerts `devices`, the list as it is now, call for.
    pub fn update(&mut self, devices: &[Peripheral]) -> Vec<Alert> {
        let mut alerts = Vec::new();
        for device in devices {
            let Some(battery) = device.battery else { continue };
            let said = self.devices.entry(key(device)).or_default();
            let percent = match battery {
                Battery::Percent(percent) => percent,
                Battery::Level(level) => level_percent(level),
            };
            if device.charging {
                said.step = None;
                continue;
            }
            while let Some(step) = said.step
                && percent >= step.saturating_add(REARM)
            {
                said.step = STEPS.iter().rev().find(|higher| **higher > step).copied();
            }
            let Some(step) = STEPS.iter().rev().find(|step| percent <= **step).copied() else { continue };
            if said.step.is_some_and(|said| said <= step) {
                continue;
            }
            said.step = Some(step);
            alerts.push(Alert { key: key(device), own: own(device, battery, step, said.notification) });
        }
        if self.devices.len() > MAX_REMEMBERED {
            self.devices.retain(|kept, _| devices.iter().any(|device| &key(device) == kept));
        }
        alerts
    }

    /// The alert for the device `key` names was sent as notification
    /// `notification`.
    pub fn sent(&mut self, key: &str, notification: u32) {
        if let Some(said) = self.devices.get_mut(key) {
            said.notification = notification;
        }
    }
}

/// What a device is known by across reconnects: what it is and its name.
/// Not its id: UPower numbers a Logitech device's battery anew each time it
/// connects (`mouse_hidpp_battery_3`).
fn key(device: &Peripheral) -> String {
    format!("{:?}/{}", device.kind, device.name)
}

/// About how full a battery that only reports `level` is.
pub fn level_percent(level: BatteryLevel) -> u8 {
    match level {
        BatteryLevel::Critical => 5,
        BatteryLevel::Low => 10,
        BatteryLevel::Normal => 50,
        BatteryLevel::High => 75,
        BatteryLevel::Full => 100,
    }
}

/// The notification for `device`, at `battery`, reaching `step`.
fn own(device: &Peripheral, battery: Battery, step: u8, replaces: u32) -> Own {
    let critical = step == STEPS[STEPS.len() - 1];
    let what = kind_name(device.kind);
    let name = &device.name;
    let summary = match (battery, critical) {
        (Battery::Percent(percent), true) => format!("{what} battery critical: {percent}%"),
        (Battery::Percent(percent), false) if step == STEPS[0] => format!("{what} battery at {percent}%"),
        (Battery::Percent(percent), false) => format!("{what} battery low: {percent}%"),
        (Battery::Level(_), true) => format!("{what} battery critical"),
        (Battery::Level(_), false) => format!("{what} battery low"),
    };
    let body = match (battery, critical) {
        (_, true) => format!("{name} will turn off soon. Charge it now."),
        (Battery::Level(_), false) => format!("{name} reports a low battery."),
        (Battery::Percent(_), false) if step == STEPS[0] => format!("{name} is running low."),
        (Battery::Percent(_), false) => format!("{name} needs charging soon."),
    };
    Own {
        app: APP.into(),
        summary,
        body,
        critical,
        replaces,
        device: Some(NotificationDevice { kind: device.kind, battery }),
    }
}

fn kind_name(kind: PeripheralKind) -> &'static str {
    match kind {
        PeripheralKind::Mouse => "Mouse",
        PeripheralKind::Keyboard => "Keyboard",
        PeripheralKind::Touchpad => "Touchpad",
        PeripheralKind::GamingInput => "Controller",
        PeripheralKind::Pen => "Pen",
        PeripheralKind::Tablet => "Tablet",
        PeripheralKind::Headset => "Headset",
        PeripheralKind::Headphones => "Headphones",
        PeripheralKind::Speakers => "Speaker",
        PeripheralKind::RemoteControl => "Remote",
        PeripheralKind::Wearable => "Wearable",
        PeripheralKind::Other => "Device",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_protocol::PeripheralSource;

    fn mouse(battery: Battery, charging: bool) -> Peripheral {
        Peripheral {
            id: "upower/mouse".into(),
            name: "MX Master 3S".into(),
            kind: PeripheralKind::Mouse,
            link: None,
            battery: Some(battery),
            charging,
            source: PeripheralSource::Upower,
        }
    }

    /// The summaries of the alerts each reading in turn calls for.
    fn summaries(readings: &[(Battery, bool)]) -> Vec<Vec<String>> {
        let mut alerts = Alerts::default();
        readings
            .iter()
            .map(|(battery, charging)| {
                alerts.update(&[mouse(*battery, *charging)]).into_iter().map(|alert| alert.own.summary).collect()
            })
            .collect()
    }

    fn percent(percent: u8) -> (Battery, bool) {
        (Battery::Percent(percent), false)
    }

    #[test]
    fn each_step_is_said_once_going_down() {
        assert_eq!(
            summaries(&[
                percent(30),
                percent(20),
                percent(19),
                percent(21),
                percent(11),
                percent(10),
                percent(6),
                percent(5)
            ]),
            [
                vec![],
                vec!["Mouse battery at 20%".to_string()],
                vec![],
                vec![],
                vec![],
                vec!["Mouse battery low: 10%".to_string()],
                vec![],
                vec!["Mouse battery critical: 5%".to_string()],
            ]
        );
    }

    #[test]
    fn a_first_low_reading_says_only_the_lowest_step() {
        assert_eq!(summaries(&[percent(3), percent(8)]), [vec!["Mouse battery critical: 3%".to_string()], vec![]]);
    }

    #[test]
    fn charging_or_a_reading_well_over_a_step_says_it_again() {
        assert_eq!(
            summaries(&[
                percent(9),
                (Battery::Percent(9), true),
                percent(9),
                percent(19),
                percent(10),
                percent(20),
                percent(10),
                percent(15),
            ]),
            [
                vec!["Mouse battery low: 9%".to_string()],
                vec![],
                vec!["Mouse battery low: 9%".to_string()],
                // 10 points over the 10% step says it again; 20% stays said.
                vec![],
                vec![],
                vec![],
                vec!["Mouse battery low: 10%".to_string()],
                vec![],
            ]
        );
    }

    #[test]
    fn a_device_back_under_a_new_id_is_still_known() {
        let mut alerts = Alerts::default();
        assert_eq!(alerts.update(&[mouse(Battery::Percent(4), false)]).len(), 1);
        let back = Peripheral { id: "upower/mouse_hidpp_battery_1".into(), ..mouse(Battery::Percent(4), false) };
        assert_eq!(alerts.update(&[back]), []);
    }

    #[test]
    fn levels_say_10_and_5() {
        let level = |level| (Battery::Level(level), false);
        assert_eq!(
            summaries(&[level(BatteryLevel::Normal), level(BatteryLevel::Low), level(BatteryLevel::Critical)]),
            [vec![], vec!["Mouse battery low".to_string()], vec!["Mouse battery critical".to_string()]]
        );
    }

    #[test]
    fn a_newer_alert_replaces_the_last_one_sent() {
        let mut alerts = Alerts::default();
        let first = alerts.update(&[mouse(Battery::Percent(20), false)]);
        assert_eq!(first[0].own.replaces, 0);
        alerts.sent(&first[0].key, 7);
        let next = alerts.update(&[mouse(Battery::Percent(10), false)]);
        assert_eq!((next[0].own.replaces, next[0].own.critical), (7, false));
        assert!(alerts.update(&[mouse(Battery::Percent(5), false)])[0].own.critical);
    }
}
