//! The peripherals feature's messages: the devices with a battery (mice,
//! keyboards, headsets...) and their charge, for quick settings.
//!
//! novad publishes an [`Event`] on [`TOPIC`] whenever a device comes, goes
//! or changes; a client sends nothing. A low battery notification carries
//! a device's kind and charge (`nova-notifications-api`'s
//! `NotificationDevice`). The work is `nova-peripherals`
//! (`features/peripherals/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Peripherals`].
pub const TOPIC: Topic = Topic::named("peripherals");

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Wireless devices and their batteries (feature `peripherals`). Topic
    /// [`TOPIC`], sent right after subscribing and whenever a
    /// device comes, goes or changes.
    Peripherals(Peripherals),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// The devices with a battery (feature `peripherals`): mice, keyboards,
/// headsets and the like that are connected and on right now and report
/// one, each listed once however many sources report it. One that's off or
/// out of reach (a headset whose dongle stays plugged in) isn't listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripherals {
    pub devices: Vec<Peripheral>,
}

/// One device and its battery. Text is plain and capped in length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripheral {
    /// Stable while the device stays, like `upower/mouse_hidpp_battery_0`
    /// or `headsetcontrol/1038:2253`.
    pub id: String,
    /// Its model, like `PRO X Wireless`.
    pub name: String,
    pub kind: PeripheralKind,
    /// How it's connected, when novad can tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<PeripheralLink>,
    /// Its charge, when it has said yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<Battery>,
    pub charging: bool,
    /// Where the reading comes from.
    pub source: PeripheralSource,
}

/// What a device is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeripheralKind {
    Mouse,
    Keyboard,
    Touchpad,
    GamingInput,
    Pen,
    Tablet,
    Headset,
    Headphones,
    Speakers,
    RemoteControl,
    Wearable,
    Other,
}

/// A device's charge: a percentage, or only a rough level for devices that
/// report no more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Battery {
    /// 0 to 100.
    Percent(u8),
    Level(BatteryLevel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryLevel {
    Critical,
    Low,
    Normal,
    High,
    Full,
}

/// How a device is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeripheralLink {
    /// Through its own USB receiver (2.4 GHz).
    Dongle,
    Bluetooth,
    /// By its cable.
    Wired,
}

/// Where novad read a device: UPower (the kernel's drivers and BlueZ),
/// HID++ (Logitech devices on a receiver the kernel doesn't drive),
/// Keychron (keyboards on firmware that reports the battery over raw HID)
/// or HeadsetControl (headsets the kernel doesn't know the battery of).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeripheralSource {
    Upower,
    Hidpp,
    Keychron,
    Headsetcontrol,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn peripherals() -> Peripherals {
        Peripherals {
            devices: vec![
                Peripheral {
                    id: "upower/mouse_hidpp_battery_0".into(),
                    name: "PRO X Wireless".into(),
                    kind: PeripheralKind::Mouse,
                    link: Some(PeripheralLink::Dongle),
                    battery: Some(Battery::Percent(72)),
                    charging: false,
                    source: PeripheralSource::Upower,
                },
                Peripheral {
                    id: "upower/keyboard_dev_AA_BB_CC_DD_EE_FF".into(),
                    name: "K380".into(),
                    kind: PeripheralKind::Keyboard,
                    link: Some(PeripheralLink::Bluetooth),
                    battery: Some(Battery::Level(BatteryLevel::High)),
                    charging: false,
                    source: PeripheralSource::Upower,
                },
                Peripheral {
                    id: "keychron/model-3".into(),
                    name: "Keychron K2 HE".into(),
                    kind: PeripheralKind::Keyboard,
                    link: Some(PeripheralLink::Dongle),
                    battery: Some(Battery::Percent(78)),
                    charging: false,
                    source: PeripheralSource::Keychron,
                },
                Peripheral {
                    id: "headsetcontrol/1038:2253".into(),
                    name: "SteelSeries Arctis Nova 5X".into(),
                    kind: PeripheralKind::Headset,
                    link: Some(PeripheralLink::Dongle),
                    battery: Some(Battery::Percent(30)),
                    charging: true,
                    source: PeripheralSource::Headsetcontrol,
                },
            ],
        }
    }

    #[test]
    fn peripherals_reject_unknown_fields() {
        let list = serde_json::to_value(peripherals()).unwrap();
        assert!(serde_json::from_value::<Peripherals>(list.clone()).is_ok());
        for pointer in ["", "/devices/0"] {
            let mut extra = list.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<Peripherals>(extra).is_err(), "{pointer}");
        }
        let mut odd = list.clone();
        odd["devices"][0]["battery"] = json!({"percent": 30, "level": "low"});
        assert!(serde_json::from_value::<Peripherals>(odd).is_err());
    }

    #[test]
    fn its_event_is_on_its_topic() {
        assert_eq!(Event::Peripherals(peripherals()).topic(), Some(TOPIC));
    }
}
