//! The peripherals feature's messages: what a device with a battery is and
//! its charge. For now only the types a low battery notification carries
//! (`nova-notifications-api`'s `NotificationDevice`); the rest of the
//! feature's messages are still `nova-protocol`'s until it moves (IMP-377).

use serde::{Deserialize, Serialize};

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
