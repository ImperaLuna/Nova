//! The recording feature's messages: picking what to record, the recording
//! itself, and the card's buttons on a saved one.
//!
//! A client sends a [`Command`] (`record`, `start_recording`,
//! `stop_recording`, `pause_recording`, `recording_action`, `open_log`);
//! novad publishes an [`Event`] on [`TOPIC`]. The card's buttons are the
//! screenshot card's ([`ScreenshotAction`]). The work is `nova-recording`
//! (`features/recording/core`).

use nova_carrier::{Published, Topic};
pub use nova_screenshot_api::ScreenshotAction;
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for the recording [`Event`]s.
pub const TOPIC: Topic = Topic::named("recording");

/// Sent by the UI or the `nova` CLI. Arguments with unknown fields are
/// refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// `nova record` (feature `recording`): stops the recording, or when
    /// nothing is being recorded asks the UI to open the region picker for
    /// one (`pick_recording`). Fails when no UI is connected for that.
    Record,
    /// Starts recording `output`, or `region` of the layout on it (the UI,
    /// after its countdown). Answers once the recorder has started.
    StartRecording {
        output: String,
        region: Option<LayoutRect>,
        #[serde(default)]
        audio: RecordAudio,
    },
    /// Stops the recording and saves it (the bar's recording pill).
    StopRecording,
    /// Pauses or resumes the recording, when the recorder can.
    PauseRecording { paused: bool },
    /// Opens, shows the folder of or deletes a recording saved since novad
    /// started (the recording card).
    RecordingAction { id: u64, action: ScreenshotAction },
    /// Opens novad's log (feature `recording`: the recording card's Open log,
    /// after a recording didn't start).
    OpenLog,
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// `nova record` while nothing is being recorded (feature `recording`):
    /// the UI opens the region picker to pick what to record, or closes it
    /// when it's open. Topic [`TOPIC`].
    PickRecording,
    /// What's being recorded, or `null` once nothing is (feature
    /// `recording`). Topic [`TOPIC`], sent right after subscribing
    /// and whenever it starts, stops or pauses.
    Recording(Option<ActiveRecording>),
    /// A recording was saved (feature `recording`). `id` names it in
    /// `recording_action`. Topic [`TOPIC`].
    RecordingSaved(SavedRecording),
    /// The recorder exited without recording anything worth keeping (no
    /// frames: a recorder stalled waiting for sound wrote only the file's
    /// header). Nothing is saved; `reason` is one sentence for the card,
    /// the rest is in the log. Topic [`TOPIC`].
    RecordingFailed { reason: String },
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// A rectangle in the compositor's layout, in logical pixels: where
/// outputs sit side by side (`screen.x` and `screen.y` in Quickshell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The sound a recording takes, mixed into one track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordAudio {
    /// What the default output plays.
    #[serde(default)]
    pub desktop: bool,
    /// The default input.
    #[serde(default)]
    pub mic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveRecording {
    /// The output it's on.
    pub output: String,
    /// The part of the layout recorded, or `null` for the whole output.
    pub region: Option<LayoutRect>,
    pub audio: RecordAudio,
    /// How much has been recorded when this was sent, pauses left out.
    pub elapsed_ms: u64,
    pub paused: bool,
    /// The recorder can pause (gpu-screen-recorder can, wf-recorder can't).
    pub can_pause: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedRecording {
    pub id: u64,
    /// Where it was saved.
    pub path: String,
    /// How long it plays.
    pub duration_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_is_the_recording_topics() {
        assert_eq!(Event::Recording(None).topic(), Some(TOPIC));
        assert_eq!(serde_json::to_string(&Event::Recording(None)).unwrap(), r#"{"event":"recording","data":null}"#);
    }
}
