//! Wire format between `novad` and the UI.
//!
//! Every message is one JSON object per line on a Unix socket. The daemon
//! pushes [`Event`]s, the UI sends [`Request`]s, and the daemon answers each
//! request that carries an `id` with a [`Response`].
//!
//! ```text
//! novad -> ui   {"v":1,"event":"hello","data":{"daemon":"0.1.0"}}
//! ui -> novad   {"v":1,"id":7,"cmd":"ping"}
//! novad -> ui   {"v":1,"id":7,"result":"pong"}
//! ```

use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change to the messages below.
pub const PROTOCOL_VERSION: u32 = 1;

/// Pushed by the daemon whenever its state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// First message on every connection.
    Hello { daemon: String },
    /// Which features are usable right now.
    Features { features: Vec<FeatureState> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureState {
    pub id: String,
    pub status: FeatureStatus,
    /// Binaries the feature needs but that aren't on `PATH`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    /// Turned on and all dependencies are present. The UI loads it.
    Ready,
    /// Turned on but a dependency is missing. The UI doesn't load it.
    MissingDeps,
    /// Turned off. The UI doesn't load it.
    Off,
}

/// Sent by the UI or the `nova` CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum Command {
    Ping,
    ListFeatures,
}

/// A message with the protocol version, flattened around its body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub v: u32,
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(body: T) -> Self {
        Self { v: PROTOCOL_VERSION, body }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Present when the sender wants a [`Response`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Result(serde_json::Value),
    Error(String),
}

/// Encodes a message as one line, newline included.
pub fn encode<T: Serialize>(body: T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&Envelope::new(body))?;
    line.push('\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every message shape on the wire. A change here changes the protocol:
    /// update `ui/daemon/protocol.js` too, then accept with `cargo insta review`.
    #[test]
    fn wire_format() {
        let lines = [
            encode(Event::Hello { daemon: "0.1.0".into() }),
            encode(Event::Features {
                features: vec![
                    FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] },
                    FeatureState {
                        id: "recording".into(),
                        status: FeatureStatus::MissingDeps,
                        missing: vec!["gpu-screen-recorder|wf-recorder".into()],
                    },
                    FeatureState { id: "weather".into(), status: FeatureStatus::Off, missing: vec![] },
                ],
            }),
            encode(Request { id: Some(7), command: Command::Ping }),
            encode(Request { id: None, command: Command::ListFeatures }),
            encode(Response { id: 7, outcome: Outcome::Result(json!("pong")) }),
            encode(Response { id: 8, outcome: Outcome::Error("bad request".into()) }),
        ];
        let all: String = lines.into_iter().map(Result::unwrap).collect();
        assert!(all.lines().all(|l| l.starts_with(r#"{"v":1,"#)));
        insta::assert_snapshot!(all);
    }

    #[test]
    fn request_without_args_parses() {
        let req: Envelope<Request> = serde_json::from_str(r#"{"v":1,"id":7,"cmd":"ping"}"#).unwrap();
        assert_eq!(req.body, Request { id: Some(7), command: Command::Ping });
    }

    #[test]
    fn request_without_id_parses() {
        let req: Envelope<Request> = serde_json::from_str(r#"{"v":1,"cmd":"list_features"}"#).unwrap();
        assert_eq!(req.body.id, None);
        assert_eq!(req.body.command, Command::ListFeatures);
    }

    #[test]
    fn unknown_command_is_rejected() {
        assert!(serde_json::from_str::<Envelope<Request>>(r#"{"v":1,"cmd":"reboot"}"#).is_err());
    }

    #[test]
    fn responses_round_trip() {
        for outcome in [Outcome::Result(json!("pong")), Outcome::Error("nope".into())] {
            let msg = Envelope::new(Response { id: 3, outcome });
            let back: Envelope<Response> = serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
            assert_eq!(back, msg);
        }
    }
}
