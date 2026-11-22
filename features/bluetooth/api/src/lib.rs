//! The Bluetooth feature's messages: the radio, the devices and the
//! pairing waiting for the user, for the quick settings Bluetooth tile and
//! page.
//!
//! A client sends a [`Command`] (`bluetooth_power`, `bluetooth_connect`,
//! ...); novad publishes an [`Event`] on [`TOPIC`] right after subscribing
//! and whenever any of it changes. The work is `nova-bluetooth`
//! (`features/bluetooth/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Bluetooth`].
pub const TOPIC: Topic = Topic::named("bluetooth");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Turns Bluetooth on or off (feature `bluetooth`). On also lifts a
    /// software block (rfkill); a hardware switch is refused.
    BluetoothPower {
        on: bool,
    },
    /// Starts or stops this connection looking for new devices, while the
    /// Bluetooth page shows (feature `bluetooth`). Scanning runs while any
    /// connection watches, for at most two minutes from the last start.
    WatchBluetooth {
        watching: bool,
    },
    /// Connects the device at `address` (feature `bluetooth`). A new one is
    /// paired and trusted first, so it reconnects by itself later; pairing
    /// may ask the user (`Bluetooth::pairing`). Answers once it's connected,
    /// or with why it failed.
    BluetoothConnect {
        address: String,
    },
    BluetoothDisconnect {
        address: String,
    },
    /// Removes the device's pairing.
    BluetoothForget {
        address: String,
    },
    /// Answers the pairing request `id`.
    BluetoothAnswer {
        id: u64,
        answer: PairingAnswer,
    },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Bluetooth: the radio, the devices and the pairing waiting for the
    /// user (feature `bluetooth`). Topic [`TOPIC`], sent right
    /// after subscribing and whenever any of it changes.
    Bluetooth(Bluetooth),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// Bluetooth as the quick settings page shows it (feature `bluetooth`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bluetooth {
    pub radio: BluetoothRadio,
    /// Looking for new devices now (`watch_bluetooth`).
    pub scanning: bool,
    /// Connected devices first, then paired ones, then the new ones nearby,
    /// each group by name. Only while the radio is on.
    pub devices: Vec<BluetoothDevice>,
    /// What pairing asks of the user now, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing: Option<PairingRequest>,
}

/// Whether Bluetooth can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothRadio {
    /// No adapter, or BlueZ isn't running.
    Missing,
    /// Off, or blocked in software: `bluetooth_power` turns it on either way.
    Off,
    /// Off by a hardware switch or key, which only the user can turn on.
    SwitchedOff,
    On,
}

/// A device the adapter knows. Text is plain and capped in length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BluetoothDevice {
    /// Its address, like `9C:2A:70:11:22:33`: the id the commands take.
    pub address: String,
    pub name: String,
    pub kind: BluetoothKind,
    pub status: BluetoothStatus,
    /// 0 to 100, when the device reports its battery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,
    /// Why the last connect, pair or disconnect failed, in words for the
    /// user. Cleared by the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What a Bluetooth device is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothKind {
    Headphones,
    Headset,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Tablet,
    Phone,
    Computer,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothStatus {
    /// Seen while scanning, never paired.
    Nearby,
    Pairing,
    /// Paired and not connected.
    Paired,
    Connecting,
    Connected,
    Disconnecting,
}

/// A question pairing asks the user, answered with `bluetooth_answer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingRequest {
    /// Names it in `bluetooth_answer`.
    pub id: u64,
    /// The device's address; it's in `devices`.
    pub address: String,
    pub ask: PairingAsk,
    /// The device started it (a phone pairing with this computer), not
    /// `bluetooth_connect`.
    pub incoming: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingAsk {
    /// Does the device show this code? `accept` or `reject`.
    Confirm { code: String },
    /// Type the PIN the device came with (old devices): answer a `code`.
    EnterPin,
    /// Type the six-digit code the device shows: answer a `code`.
    EnterPasskey,
    /// Type this PIN on the device. Needs no answer.
    ShowPin { pin: String },
    /// Type this code on the device; `typed` digits are in so far. Needs no
    /// answer.
    ShowPasskey { code: String, typed: u8 },
    /// May this device pair (or use a service)? `accept` or `reject`.
    Authorize,
}

/// The user's answer to a [`PairingRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingAnswer {
    Accept,
    Reject,
    /// The PIN or passkey typed.
    Code(String),
}

/// The longest PIN a `bluetooth_answer` takes, in bytes (BlueZ's limit).
pub const BLUETOOTH_PIN_MAX_BYTES: usize = 16;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bluetooth() -> Bluetooth {
        Bluetooth {
            radio: BluetoothRadio::On,
            scanning: true,
            devices: vec![
                BluetoothDevice {
                    address: "00:1B:66:AA:BB:CC".into(),
                    name: "WH-1000XM5".into(),
                    kind: BluetoothKind::Headphones,
                    status: BluetoothStatus::Connected,
                    battery: Some(80),
                    error: None,
                },
                BluetoothDevice {
                    address: "9C:2A:70:11:22:33".into(),
                    name: "Pixel 8".into(),
                    kind: BluetoothKind::Phone,
                    status: BluetoothStatus::Pairing,
                    battery: None,
                    error: None,
                },
                BluetoothDevice {
                    address: "C4:30:18:44:55:66".into(),
                    name: "JBL Flip 6".into(),
                    kind: BluetoothKind::Speaker,
                    status: BluetoothStatus::Nearby,
                    battery: None,
                    error: Some("Couldn't reach it. Make sure it's on and nearby.".into()),
                },
            ],
            pairing: Some(PairingRequest {
                id: 3,
                address: "9C:2A:70:11:22:33".into(),
                ask: PairingAsk::Confirm { code: "482913".into() },
                incoming: false,
            }),
        }
    }

    #[test]
    fn bluetooth_rejects_unknown_fields() {
        let state = serde_json::to_value(bluetooth()).unwrap();
        assert!(serde_json::from_value::<Bluetooth>(state.clone()).is_ok());
        for pointer in ["", "/devices/0", "/pairing", "/pairing/ask"] {
            let mut extra = state.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<Bluetooth>(extra).is_err(), "{pointer}");
        }
        let answer = |value| serde_json::from_value::<PairingAnswer>(value).ok();
        assert_eq!(answer(json!("accept")), Some(PairingAnswer::Accept));
        assert_eq!(answer(json!({"code": "0000"})), Some(PairingAnswer::Code("0000".into())));
        assert_eq!(answer(json!({"code": "0000", "extra": 1})), None);
        assert_eq!(answer(json!("maybe")), None);
    }

    #[test]
    fn its_event_is_on_its_topic() {
        assert_eq!(Event::Bluetooth(bluetooth()).topic(), Some(TOPIC));
    }
}
