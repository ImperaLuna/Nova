//! Low battery alerts: a notification when a device's battery drops to
//! [`STEPS`] (20%, 10%, then 5%, critical), each once per discharge.
//!
//! A device read for the first time already low gets one alert, for the
//! lowest step it's under. Charging says them all again next time; so does
//! a reading [`REARM`] points over a step (a device that comes back
//! charged), for that step and the ones under it, so a reading wobbling
//! around a step doesn't say it twice. A device that only reports a level
//! reads as [`level_percent`]: `low` is the 10% alert, `critical` the 5%
//! one. What's been said is kept by device id while novad runs, also while
//! a device is away, so one that reconnects still low isn't alerted again;
//! one back under a new id takes over what was said under an id that's gone
//! for the same device ([`same_device`]): a headset whose Bluetooth drops
//! while its dongle carries on is still the headset.
//!
//! An alert is the device's name and its percent, and takes the place of
//! the device's last one; after a restart, of its row still in the list
//! ([`row_of`]).

use super::{names_match, same_kind};
use crate::notifications::Own;
use nova_protocol::{
    Battery, BatteryLevel, Notification, NotificationDevice, Peripheral, PeripheralKind, PeripheralLink,
};
use std::collections::HashMap;

/// The percents alerted at, highest first; the last one is critical.
pub const STEPS: [u8; 3] = [20, 10, 5];
/// How far over a step a reading has to go to say it again.
pub const REARM: u8 = 10;
/// Devices remembered; past it, ones not connected now are forgotten.
const MAX_REMEMBERED: usize = 64;
/// Who the alerts read as from.
pub const APP: &str = "Devices";

/// What's been said about each device, by id.
#[derive(Debug, Default)]
pub struct Alerts {
    devices: HashMap<String, Said>,
}

#[derive(Debug, Clone)]
struct Said {
    /// What the device is, its name and whether it's on Bluetooth, to
    /// know it under another id.
    kind: PeripheralKind,
    name: String,
    bluetooth: bool,
    /// The lowest step alerted this discharge.
    step: Option<u8>,
    /// The device's last alert, which its next one takes the place of.
    notification: u32,
}

/// An alert to send for device `id`.
#[derive(Debug, PartialEq)]
pub struct Alert {
    pub id: String,
    pub own: Own,
}

impl Alerts {
    /// The alerts `devices`, the list as it is now, call for.
    pub fn update(&mut self, devices: &[Peripheral]) -> Vec<Alert> {
        let mut alerts = Vec::new();
        for device in devices {
            let Some(battery) = device.battery else { continue };
            if !self.devices.contains_key(&device.id) {
                let away = self.devices.iter().find(|(id, said)| {
                    same_device(said, device) && !devices.iter().any(|connected| &connected.id == *id)
                });
                let said = away.map(|(id, _)| id.clone()).and_then(|id| self.devices.remove(&id));
                let mut said = said.unwrap_or(Said {
                    kind: device.kind,
                    name: String::new(),
                    bluetooth: false,
                    step: None,
                    notification: 0,
                });
                said.kind = device.kind;
                said.name.clone_from(&device.name);
                said.bluetooth = on_bluetooth(device);
                self.devices.insert(device.id.clone(), said);
            }
            let Some(said) = self.devices.get_mut(&device.id) else { continue };
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
            alerts.push(Alert { id: device.id.clone(), own: own(device, battery, step, said.notification) });
        }
        if self.devices.len() > MAX_REMEMBERED {
            self.devices.retain(|id, _| devices.iter().any(|device| &device.id == id));
        }
        alerts
    }

    /// The notifications sent for the devices known, by id.
    pub fn notifications(&self) -> Vec<u32> {
        self.devices.values().map(|said| said.notification).filter(|id| *id != 0).collect()
    }

    /// Device `id`'s alert was sent as notification `notification`.
    pub fn sent(&mut self, id: &str, notification: u32) {
        if let Some(said) = self.devices.get_mut(id) {
            said.notification = notification;
        }
    }
}

/// Whether what was `said` is about `device` back under a new id: UPower
/// numbers a Logitech battery anew each time it connects
/// (`mouse_hidpp_battery_3`), and a headset on Bluetooth and its dongle is
/// listed as the dongle's (`Arctis Nova 5X`) while Bluetooth is away. The
/// same name, or the way the list merges a device on Bluetooth and its
/// dongle: one of them on Bluetooth, and one name inside the other.
fn same_device(said: &Said, device: &Peripheral) -> bool {
    let names =
        said.name == device.name || (said.bluetooth != on_bluetooth(device) && names_match(&said.name, &device.name));
    same_kind(said.kind, device.kind) && names
}

fn on_bluetooth(device: &Peripheral) -> bool {
    device.link == Some(PeripheralLink::Bluetooth)
}

/// Whether `listed` is the row an earlier alert like `own` left: Nova's,
/// about a device of its kind and the same name. A row doesn't say how its
/// device was connected, so a name inside another's isn't enough.
pub fn row_of(listed: &Notification, own: &Own) -> bool {
    let kinds = listed.device.zip(own.device).is_some_and(|(listed, own)| same_kind(listed.kind, own.kind));
    listed.app == own.app && kinds && listed.summary == own.summary
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

/// The notification for `device`, at `battery`, reaching `step`: its name
/// and its percent. The icon says what it is; when critical, the card's
/// rim says to charge it.
fn own(device: &Peripheral, battery: Battery, step: u8, replaces: u32) -> Own {
    let critical = step == STEPS[STEPS.len() - 1];
    let body = match (battery, critical) {
        (Battery::Percent(percent), _) => format!("{percent}%"),
        (Battery::Level(_), true) => "Critical".into(),
        (Battery::Level(_), false) => "Low".into(),
    };
    Own {
        app: APP.into(),
        summary: device.name.clone(),
        body,
        critical,
        replaces,
        device: Some(NotificationDevice { kind: device.kind, battery }),
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

    /// What the alerts each reading in turn calls for say.
    fn bodies(readings: &[(Battery, bool)]) -> Vec<Vec<String>> {
        let mut alerts = Alerts::default();
        readings
            .iter()
            .map(|(battery, charging)| {
                alerts.update(&[mouse(*battery, *charging)]).into_iter().map(|alert| alert.own.body).collect()
            })
            .collect()
    }

    fn percent(percent: u8) -> (Battery, bool) {
        (Battery::Percent(percent), false)
    }

    #[test]
    fn each_step_is_said_once_going_down() {
        assert_eq!(
            bodies(&[
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
                vec!["20%".to_string()],
                vec![],
                vec![],
                vec![],
                vec!["10%".to_string()],
                vec![],
                vec!["5%".to_string()],
            ]
        );
    }

    #[test]
    fn a_first_low_reading_says_only_the_lowest_step() {
        assert_eq!(bodies(&[percent(3), percent(8)]), [vec!["3%".to_string()], vec![]]);
    }

    #[test]
    fn charging_or_a_reading_well_over_a_step_says_it_again() {
        assert_eq!(
            bodies(&[
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
                vec!["9%".to_string()],
                vec![],
                vec!["9%".to_string()],
                // 10 points over the 10% step says it again; 20% stays said.
                vec![],
                vec![],
                vec![],
                vec!["10%".to_string()],
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
    fn two_of_the_same_model_are_two_devices() {
        let mut alerts = Alerts::default();
        let other = |battery| Peripheral { id: "upower/mouse_2".into(), ..mouse(battery, false) };
        let both = |a, b| [mouse(Battery::Percent(a), false), other(Battery::Percent(b))];
        assert_eq!(alerts.update(&both(4, 80)).len(), 1);
        assert_eq!(alerts.update(&both(4, 79)), [], "the charged one doesn't say the other's again");
        assert_eq!(alerts.update(&both(4, 19)).len(), 1);
    }

    #[test]
    fn levels_say_10_and_5() {
        let level = |level| (Battery::Level(level), false);
        assert_eq!(
            bodies(&[level(BatteryLevel::Normal), level(BatteryLevel::Low), level(BatteryLevel::Critical)]),
            [vec![], vec!["Low".to_string()], vec!["Critical".to_string()]]
        );
    }

    #[test]
    fn a_newer_alert_replaces_the_last_one_sent() {
        let mut alerts = Alerts::default();
        let first = alerts.update(&[mouse(Battery::Percent(20), false)]);
        assert_eq!(first[0].own.replaces, 0);
        alerts.sent(&first[0].id, 7);
        let next = alerts.update(&[mouse(Battery::Percent(10), false)]);
        assert_eq!((next[0].own.replaces, next[0].own.critical), (7, false));
        assert!(alerts.update(&[mouse(Battery::Percent(5), false)])[0].own.critical);
    }

    /// The Arctis Nova 5X on Bluetooth and its dongle is listed as UPower's
    /// while Bluetooth is there, as HeadsetControl's while it's away.
    #[test]
    fn a_headset_on_bluetooth_and_its_dongle_is_one_device() {
        let bluetooth = |percent| Peripheral {
            id: "upower/headphones_dev_20_AF_1B_0A_FD_79".into(),
            name: "SteelSeries Arctis Nova 5X".into(),
            kind: PeripheralKind::Headphones,
            link: Some(PeripheralLink::Bluetooth),
            battery: Some(Battery::Percent(percent)),
            charging: false,
            source: PeripheralSource::Upower,
        };
        let dongle = |percent| Peripheral {
            id: "headsetcontrol/1038:2253".into(),
            name: "Arctis Nova 5X".into(),
            kind: PeripheralKind::Headset,
            link: Some(PeripheralLink::Dongle),
            battery: Some(Battery::Percent(percent)),
            charging: false,
            source: PeripheralSource::Headsetcontrol,
        };
        let mut alerts = Alerts::default();
        let first = alerts.update(&[bluetooth(9)]);
        assert_eq!(
            first.iter().map(|alert| alert.own.summary.as_str()).collect::<Vec<_>>(),
            ["SteelSeries Arctis Nova 5X"]
        );
        alerts.sent(&first[0].id, 7);
        assert_eq!(alerts.update(&[dongle(10)]), [], "Bluetooth away: the dongle's reading is the same headset's");
        assert_eq!(alerts.update(&[bluetooth(9)]), [], "and back");
        let critical = alerts.update(&[dongle(5)]);
        assert_eq!((critical[0].own.replaces, critical[0].own.body.as_str()), (7, "5%"));
    }

    /// A name inside another's is one device only across Bluetooth and a
    /// dongle: an MX Keys Mini isn't the MX Keys that left.
    #[test]
    fn another_device_with_a_longer_name_is_another_device() {
        let keyboard = |id: &str, name: &str, percent| Peripheral {
            id: id.into(),
            name: name.into(),
            kind: PeripheralKind::Keyboard,
            battery: Some(Battery::Percent(percent)),
            ..mouse(Battery::Percent(percent), false)
        };
        let mut alerts = Alerts::default();
        assert_eq!(alerts.update(&[keyboard("upower/keyboard_0", "MX Keys", 10)]).len(), 1);
        assert_eq!(alerts.update(&[keyboard("upower/keyboard_1", "MX Keys Mini", 8)]).len(), 1);
    }
}
