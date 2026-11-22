//! The screenshot feature's messages: a region the user picks or the whole
//! focused screen, and the card's buttons on a saved shot.
//!
//! A client sends a [`Command`] (`screenshot`, `save_capture`,
//! `cancel_capture`, `screenshot_action`); novad publishes an [`Event`] on
//! [`TOPIC`]. [`ScreenshotAction`] is the recording card's too. The work is
//! `nova-screenshot` (`features/screenshot/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for the screenshot [`Event`]s.
pub const TOPIC: Topic = Topic::named("screenshot");

/// Sent by the UI or the `nova` CLI. Arguments with unknown fields are
/// refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Takes a screenshot (`nova screenshot`, feature `screenshot`). `region`
    /// freezes the screens and answers once the picker can show; asked again
    /// while it shows, it closes it. `screen` saves the focused screen.
    Screenshot { mode: CaptureMode },
    /// Saves `rect` of `output`'s frozen frame, in that frame's pixels, and
    /// closes the picker (the UI).
    SaveCapture { output: String, rect: PixelRect },
    /// Closes the region picker without a screenshot (the UI).
    CancelCapture,
    /// Opens, shows the folder of or deletes a screenshot saved since novad
    /// started (the screenshot card).
    ScreenshotAction { id: u64, action: ScreenshotAction },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The screens are frozen for a region screenshot (feature `screenshot`):
    /// each frame is what one output showed, for the UI to draw under the
    /// region picker. Topic [`TOPIC`].
    Capture { frames: Vec<CaptureFrame> },
    /// The region picker closes: the shot was taken or cancelled. Topic
    /// [`TOPIC`].
    CaptureEnded,
    /// A screenshot was saved, and copied when it could be (feature
    /// `screenshot`). `id` names it in `screenshot_action`. Topic
    /// [`TOPIC`].
    ScreenshotSaved(SavedScreenshot),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// What `nova screenshot` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Freezes every screen and lets the user drag out a region.
    Region,
    /// The whole focused screen, right away.
    Screen,
}

/// One frozen screen, as `grim` saved it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureFrame {
    /// The output's connector name (`DP-1`), as Quickshell's `screen.name`.
    pub output: String,
    /// A PNG in novad's private runtime folder, gone once the picker closes.
    pub path: String,
}

/// A rectangle in an image's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedScreenshot {
    pub id: u64,
    /// Where it was saved.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// It's on the clipboard too (wl-copy worked).
    pub copied: bool,
}

/// What the screenshot or recording card does with a saved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotAction {
    /// Opens it in the default app (image viewer, video player).
    Open,
    /// Opens the folder it's in.
    Folder,
    /// Deletes the file.
    Delete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_is_the_screenshot_topics() {
        assert_eq!(Event::CaptureEnded.topic(), Some(TOPIC));
        assert_eq!(serde_json::to_string(&Event::CaptureEnded).unwrap(), r#"{"event":"capture_ended"}"#);
    }
}
