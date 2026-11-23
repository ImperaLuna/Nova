//! `nova volume`'s messages on novad's socket. novad passes the key on to
//! the UI, which shows the capsule. Quickshell's shell changes the volume
//! itself (PipeWire through Quickshell); for the GPUI UI novad changes it,
//! through the sound feature (`nova_sound::Sound::press`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Volume`].
pub const TOPIC: Topic = Topic::named("volume");

/// Sent by the `nova` CLI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Changes the default output's volume and shows the capsule, even at
    /// either end or without an output (`nova volume`). Fails when no UI is
    /// connected to do it.
    Volume { key: VolumeKey },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Show the volume capsule, and in Quickshell's shell change the
    /// default output's volume as `nova volume` asks.
    Volume { key: VolumeKey },
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// A volume key `nova volume` stands for. A step is
/// `nova_sound_api::VOLUME_STEP` (`Tokens.volumeStep`), and stops at 100%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeKey {
    /// One step louder.
    Up,
    /// One step quieter.
    Down,
    /// Mute, or unmute when muted.
    Mute,
}
