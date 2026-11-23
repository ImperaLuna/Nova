//! What `devices.js` worked out for the Devices card, with no GPUI drawing
//! in it: each device's icon, how its link reads, how full its battery is
//! drawn, when it reads as low, and the text beside it.

use nova_look::Glyph;
use nova_peripherals_api::{Battery, BatteryLevel, PeripheralKind, PeripheralLink};

/// At or under this percent a battery reads as low.
pub const LOW_PERCENT: u8 = 15;

pub fn icon(kind: PeripheralKind) -> Glyph {
    match kind {
        PeripheralKind::Mouse | PeripheralKind::Touchpad => Glyph::MOUSE,
        PeripheralKind::Keyboard => Glyph::KEYBOARD,
        PeripheralKind::GamingInput => Glyph::GAMEPAD,
        PeripheralKind::Headset | PeripheralKind::Headphones => Glyph::HEADPHONES,
        _ => Glyph::DEVICE,
    }
}

pub fn link_icon(link: PeripheralLink) -> Glyph {
    match link {
        PeripheralLink::Dongle => Glyph::DONGLE,
        PeripheralLink::Bluetooth => Glyph::BLUETOOTH,
        PeripheralLink::Wired => Glyph::PLUG,
    }
}

pub fn link_label(link: PeripheralLink) -> &'static str {
    match link {
        PeripheralLink::Dongle => "2.4 GHz",
        PeripheralLink::Bluetooth => "Bluetooth",
        PeripheralLink::Wired => "Wired",
    }
}

/// How full its battery is drawn, 0 to 1 (0 when it hasn't said).
pub fn fill(battery: Option<Battery>) -> f32 {
    match battery {
        None => 0.0,
        Some(Battery::Percent(percent)) => f32::from(percent.min(100)) / 100.0,
        Some(Battery::Level(level)) => match level {
            BatteryLevel::Critical => 0.05,
            BatteryLevel::Low => 0.25,
            BatteryLevel::Normal => 0.5,
            BatteryLevel::High => 0.75,
            BatteryLevel::Full => 1.0,
        },
    }
}

/// Low, unless it charges.
pub fn low(battery: Option<Battery>, charging: bool) -> bool {
    match battery {
        _ if charging => false,
        None => false,
        Some(Battery::Percent(percent)) => percent <= LOW_PERCENT,
        Some(Battery::Level(level)) => matches!(level, BatteryLevel::Critical | BatteryLevel::Low),
    }
}

/// What the text beside the battery says, and how it is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A percentage.
    Plain,
    /// A rough level, or nothing said yet.
    Soft,
    Low,
}

pub fn amount(battery: Option<Battery>, charging: bool) -> (String, Tone) {
    let tone = |plain| if low(battery, charging) { Tone::Low } else { plain };
    match battery {
        None => ("—".to_owned(), Tone::Soft),
        Some(Battery::Percent(percent)) => (format!("{percent}%"), tone(Tone::Plain)),
        Some(Battery::Level(level)) => {
            let text = match level {
                BatteryLevel::Critical => "Critical",
                BatteryLevel::Low => "Low",
                BatteryLevel::Normal => "Normal",
                BatteryLevel::High => "High",
                BatteryLevel::Full => "Full",
            };
            (text.to_owned(), tone(Tone::Soft))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_battery_is_low_at_fifteen_percent_or_a_low_step_unless_charging() {
        assert!(low(Some(Battery::Percent(15)), false));
        assert!(!low(Some(Battery::Percent(16)), false));
        assert!(!low(Some(Battery::Percent(5)), true));
        assert!(low(Some(Battery::Level(BatteryLevel::Critical)), false));
        assert!(!low(Some(Battery::Level(BatteryLevel::Normal)), false));
        assert!(!low(None, false));
    }

    #[test]
    fn a_step_is_drawn_part_full() {
        assert_eq!(fill(Some(Battery::Percent(40))), 0.4);
        assert_eq!(fill(Some(Battery::Level(BatteryLevel::High))), 0.75);
        assert_eq!(fill(None), 0.0);
    }

    #[test]
    fn the_text_beside_the_battery() {
        assert_eq!(amount(Some(Battery::Percent(80)), false), ("80%".into(), Tone::Plain));
        assert_eq!(amount(Some(Battery::Percent(8)), false), ("8%".into(), Tone::Low));
        assert_eq!(amount(Some(Battery::Level(BatteryLevel::High)), false), ("High".into(), Tone::Soft));
        assert_eq!(amount(Some(Battery::Level(BatteryLevel::Low)), false), ("Low".into(), Tone::Low));
        assert_eq!(amount(None, false), ("—".into(), Tone::Soft));
    }
}
