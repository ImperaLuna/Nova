use super::{names_match, same_kind};
use nova_notifications_api::{Notification, NotificationDevice, Own};
use nova_peripherals_api::{Battery, BatteryLevel, Peripheral, PeripheralKind, PeripheralLink};
use std::collections::HashMap;

pub const STEPS: [u8; 3] = [20, 10, 5];
pub const REARM: u8 = 10;
const MAX_REMEMBERED: usize = 64;
pub const APP: &str = "Devices";

#[derive(Debug, Default)]
pub struct Alerts {
    devices: HashMap<String, Said>,
}

#[derive(Debug, Clone)]
struct Said {
    kind: PeripheralKind,
    name: String,
    bluetooth: bool,
    step: Option<u8>,
    notification: u32,
}

#[derive(Debug, PartialEq)]
pub struct Alert {
    pub id: String,
    pub own: Own,
}

impl Alerts {
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

    pub fn notifications(&self) -> Vec<u32> {
        self.devices.values().map(|said| said.notification).filter(|id| *id != 0).collect()
    }

    pub fn sent(&mut self, id: &str, notification: u32) {
        if let Some(said) = self.devices.get_mut(id) {
            said.notification = notification;
        }
    }
}

fn same_device(said: &Said, device: &Peripheral) -> bool {
    let names =
        said.name == device.name || (said.bluetooth != on_bluetooth(device) && names_match(&said.name, &device.name));
    same_kind(said.kind, device.kind) && names
}

fn on_bluetooth(device: &Peripheral) -> bool {
    device.link == Some(PeripheralLink::Bluetooth)
}

pub fn row_of(listed: &Notification, own: &Own) -> bool {
    let kinds = listed.device.zip(own.device).is_some_and(|(listed, own)| same_kind(listed.kind, own.kind));
    listed.app == own.app && kinds && listed.summary == own.summary
}

pub fn level_percent(level: BatteryLevel) -> u8 {
    match level {
        BatteryLevel::Critical => 5,
        BatteryLevel::Low => 10,
        BatteryLevel::Normal => 50,
        BatteryLevel::High => 75,
        BatteryLevel::Full => 100,
    }
}

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
    use nova_peripherals_api::PeripheralSource;

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
