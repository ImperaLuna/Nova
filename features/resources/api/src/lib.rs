//! The resources feature's messages: what the machine is busy with (CPU,
//! memory, GPU, the CPU's temperature and the apps using the most memory),
//! for the quick settings resources card and page.
//!
//! A client sends a [`Command`] (`watch_resources`, `end_app`); while some
//! connection watches, novad publishes an [`Event`] on [`TOPIC`] every few
//! seconds. The work is `nova-resources` (`features/resources/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Resources`].
pub const TOPIC: Topic = Topic::named("resources");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Starts or stops this connection watching the machine's resources:
    /// while any connection watches, novad samples them every few seconds
    /// and sends `resources` events. A connection that closes stops
    /// watching.
    WatchResources { watching: bool },
    /// Asks every process of an app in the latest `resources` event to
    /// close (SIGTERM), by its [`AppUsage::key`]. Only the user's own
    /// processes, and never a [`AppUsage::protected`] app.
    EndApp { key: String },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// What the machine is busy with, sent every few seconds while some
    /// client watches (`watch_resources`), and right when one starts to.
    Resources(Resources),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// One look at what the machine is busy with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    /// Share of all CPU time in use since the last look, 0 to 100.
    pub cpu: f32,
    /// Memory in use (not available to start programs), in bytes.
    pub memory_used: u64,
    pub memory_total: u64,
    /// Share of the GPU in use, 0 to 100, when the driver reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f32>,
    /// The CPU's temperature in °C, when a sensor reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// CPU and memory over the last few minutes, oldest first, one sample
    /// every [`RESOURCE_HISTORY_INTERVAL_SECS`].
    pub history: Vec<ResourceSample>,
    /// How many samples `history` holds once it's full.
    pub history_size: usize,
    /// The apps using the most memory, most first. An app is every process
    /// running the same program.
    pub apps: Vec<AppUsage>,
}

/// Seconds between two of [`Resources::history`]'s samples.
pub const RESOURCE_HISTORY_INTERVAL_SECS: u64 = 5;

/// CPU and memory at one point in time, each 0 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSample {
    pub cpu: f32,
    pub memory: f32,
}

/// One app's processes together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppUsage {
    /// The program's name, from its file (`.firefox-wrapped` is `firefox`);
    /// `Nova` for novad and its UI.
    pub name: String,
    /// What `end_app` takes: the program's path, `comm:<name>` for a
    /// program novad can't see, or `nova` for Nova.
    pub key: String,
    /// How many processes run it.
    pub processes: u32,
    /// Their share of all CPU time since the last look, 0 to 100.
    pub cpu: f32,
    /// Their resident memory together, in bytes.
    pub memory: u64,
    /// Ending it isn't offered: Nova itself, the compositor, or another
    /// user's.
    pub protected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources() -> Resources {
        Resources {
            cpu: 23.5,
            memory_used: 10_093_173_760,
            memory_total: 34_359_738_368,
            gpu: Some(18.0),
            temperature: None,
            history: vec![ResourceSample { cpu: 21.0, memory: 29.0 }, ResourceSample { cpu: 23.5, memory: 29.4 }],
            history_size: 36,
            apps: vec![AppUsage {
                name: "firefox".into(),
                key: "/usr/lib/firefox/firefox".into(),
                processes: 9,
                cpu: 6.1,
                memory: 2_400_000_000,
                protected: false,
            }],
        }
    }

    #[test]
    fn resources_reject_unknown_fields() {
        fn with_extra(value: serde_json::Value, pointer: &str) -> serde_json::Value {
            let mut value = value;
            value.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            value
        }
        let resources = serde_json::to_value(resources()).unwrap();
        assert!(serde_json::from_value::<Resources>(resources.clone()).is_ok());
        for pointer in ["", "/history/0", "/apps/0"] {
            assert!(serde_json::from_value::<Resources>(with_extra(resources.clone(), pointer)).is_err(), "{pointer}");
        }
    }

    #[test]
    fn its_event_is_on_its_topic() {
        assert_eq!(Event::Resources(resources()).topic(), Some(TOPIC));
    }
}
