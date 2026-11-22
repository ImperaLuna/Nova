//! The sound feature's messages: every output and input, the apps playing,
//! their volume and mute, which output and input are in use and who records
//! from the mic, for the volume capsule and quick settings' sliders and
//! Sound page.
//!
//! novad publishes an [`Event`] on [`TOPIC`] whenever any of it changes,
//! whoever changed it, and to a client right as it subscribes. The work is
//! `nova-sound` (`features/sound/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Sound`].
pub const TOPIC: Topic = Topic::named("sound");

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Everything about sound, sent when any of it changes and to a client
    /// as it subscribes.
    Sound(Sound),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// Everything about sound right now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sound {
    /// Whether the sound server answers. While it doesn't, the lists are
    /// empty.
    pub available: bool,
    /// Where sound can play. The one with [`Device::current`] is in use;
    /// none is while there is no output.
    pub outputs: Vec<Device>,
    /// Where sound can be recorded from (mics), the one in use marked the
    /// same way.
    pub inputs: Vec<Device>,
    /// The apps playing sound, one entry each however many streams an app
    /// has.
    pub apps: Vec<App>,
    /// The apps recording from the input in use, by name, each once.
    pub recorders: Vec<String>,
}

/// An output or an input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    /// Tells it from the others. It changes when the device goes and comes
    /// back.
    pub id: u32,
    /// Its name as people know it.
    pub name: String,
    pub kind: DeviceKind,
    /// How it's connected, when that tells it apart from the same device's
    /// other link (a headset on Bluetooth and on its USB dongle has one
    /// name for both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
    /// 1 is 100%; another program can set more.
    pub volume: f32,
    pub muted: bool,
    /// The one in use: what plays, or what records, unless an app picked
    /// another.
    pub current: bool,
}

/// What a device is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// Headphones or a headset, however they're connected.
    Headphones,
    /// Another Bluetooth device, like a speaker.
    Bluetooth,
    /// A monitor or TV, over HDMI or DisplayPort.
    Display,
    /// Anything else. Nothing more is guessed: a USB output stays speakers.
    Speakers,
}

/// How a device is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Bluetooth,
    /// A cable or a wireless dongle: the sound server can't tell them apart.
    Usb,
}

/// An app playing sound: all its streams together, since an app often has
/// more than one (a browser's tabs) and each shows as the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    /// Tells it from the others: its first stream's id.
    pub id: u32,
    pub name: String,
    /// Its loudest stream's volume. 1 is 100%.
    pub volume: f32,
    /// Muted only while all its streams are.
    pub muted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound() -> Sound {
        Sound {
            available: true,
            outputs: vec![Device {
                id: 52,
                name: "Arctis Nova 7".into(),
                kind: DeviceKind::Headphones,
                link: Some(Link::Usb),
                volume: 0.4,
                muted: false,
                current: true,
            }],
            inputs: Vec::new(),
            apps: vec![App { id: 77, name: "Firefox".into(), volume: 1.0, muted: false }],
            recorders: vec!["Discord".into()],
        }
    }

    #[test]
    fn sound_rejects_unknown_fields() {
        fn with_extra(value: serde_json::Value, pointer: &str) -> serde_json::Value {
            let mut value = value;
            value.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            value
        }
        let sound = serde_json::to_value(sound()).unwrap();
        assert!(serde_json::from_value::<Sound>(sound.clone()).is_ok());
        for pointer in ["", "/outputs/0", "/apps/0"] {
            assert!(serde_json::from_value::<Sound>(with_extra(sound.clone(), pointer)).is_err(), "{pointer}");
        }
    }

    #[test]
    fn a_device_without_a_link_says_nothing_about_it() {
        let device = Device { link: None, ..sound().outputs.remove(0) };
        let value = serde_json::to_value(&device).unwrap();
        assert!(value.get("link").is_none(), "{value}");
        assert_eq!(serde_json::from_value::<Device>(value).unwrap(), device);
    }

    #[test]
    fn its_event_is_on_its_topic() {
        assert_eq!(Event::Sound(sound()).topic(), Some(TOPIC));
    }
}
