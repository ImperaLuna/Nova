//! Every topic novad serves, by its owner's constant: the shared ones in
//! `nova_protocol::topics`, a moved feature's in its api crate. A client may
//! subscribe only to these (`subscribe`'s names are checked against
//! [`TOPICS`] as the request is read), and `ui/daemon/protocol.js` names
//! them in this order.
//!
//! A feature that moves its topic into its api crate replaces the shared
//! constant here with its own, in the same place, so nothing else moves.

use nova_protocol::{Topic, topics};
use std::sync::LazyLock;

/// The topics, in protocol.js's order.
pub const TOPICS: &[Topic] = &[
    topics::FEATURES,
    topics::LOGS,
    topics::THEME,
    topics::AI_USAGE,
    topics::COMPOSITOR,
    topics::TOGGLE,
    topics::LOCK,
    topics::VOLUME,
    topics::SCREENSHOT,
    topics::RECORDING,
    topics::HOST,
    nova_resources_api::TOPIC,
    topics::LAUNCHER,
    nova_tray_api::TOPIC,
    topics::NOTIFICATIONS,
    topics::MEDIA,
    topics::PERIPHERALS,
    nova_network_api::TOPIC,
    nova_bluetooth_api::TOPIC,
];

/// Their wire names, for serde's "unknown variant" error.
static NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| TOPICS.iter().map(|topic| topic.name()).collect());

/// The wire names of [`TOPICS`], in order.
pub fn names() -> &'static [&'static str] {
    &NAMES
}

/// The topic called `name`, or serde's "unknown variant" error naming
/// every topic, as a derived enum of them would give.
pub fn named<E: serde::de::Error>(name: &str) -> Result<Topic, E> {
    TOPICS.iter().copied().find(|topic| topic.name() == name).ok_or_else(|| E::unknown_variant(name, names()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_is_one_topic() {
        let mut seen = std::collections::HashSet::new();
        for name in names() {
            assert!(seen.insert(name), "two topics are both called {name:?}");
        }
    }

    #[test]
    fn an_unknown_name_gets_serdes_error() {
        let error = named::<serde_json::Error>("weather").unwrap_err().to_string();
        assert!(error.starts_with("unknown variant `weather`, expected one of `features`, `logs`, `theme`"), "{error}");
        assert_eq!(named::<serde_json::Error>("resources").unwrap(), nova_resources_api::TOPIC);
    }
}
