//! The envelope around every message: one JSON object per line, with the
//! protocol version `v`. A request carries an optional `id`, its `cmd` and
//! its `args`; the answer to one with an `id` carries the same `id` and a
//! `result` or an `error`. Events carry `event` and `data` (see
//! [`crate::topic`]).
//!
//! ```text
//! client -> novad   {"v":2,"id":1,"cmd":"subscribe","args":{"topics":["features"]}}
//! novad -> client   {"v":2,"id":1,"result":null}
//! ```

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change to the messages.
pub const PROTOCOL_VERSION: u32 = 2;

/// The longest request line novad reads: a longer one closes the
/// connection, so a client splits a long list across requests.
pub const REQUEST_MAX_BYTES: u64 = 64 * 1024;

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

/// A request for the command type `C`, a `cmd`-tagged enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request<C> {
    /// Present when the sender wants a [`Response`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub command: C,
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

/// A request line exactly as it may arrive: no other fields. `Envelope` and
/// `Request` flatten their bodies, and serde can't reject unknown fields
/// through `flatten`, so incoming lines are checked against this first.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestLine {
    v: u32,
    #[serde(default)]
    id: Option<u64>,
    cmd: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

/// A request line that passed the envelope's checks, before its command is
/// read as any type.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    pub id: Option<u64>,
    pub cmd: String,
    pub args: Option<serde_json::Value>,
}

/// Reads one request line's envelope. Anything on the socket is untrusted:
/// unknown fields and other protocol versions are errors.
pub fn read_request(line: &str) -> Result<RawRequest, String> {
    let RequestLine { v, id, cmd, args } = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if v != PROTOCOL_VERSION {
        return Err(format!("protocol version {v} isn't supported; this daemon speaks {PROTOCOL_VERSION}"));
    }
    Ok(RawRequest { id, cmd, args })
}

impl RawRequest {
    /// Reads the command as `C`: an unknown command or bad arguments are
    /// errors.
    pub fn decode<C: DeserializeOwned>(self) -> Result<Request<C>, String> {
        let mut tagged = serde_json::Map::new();
        tagged.insert("cmd".into(), self.cmd.into());
        if let Some(args) = self.args {
            tagged.insert("args".into(), args);
        }
        let command = serde_json::from_value(tagged.into()).map_err(|e| e.to_string())?;
        Ok(Request { id: self.id, command })
    }
}

/// Reads one request line from a client as a command of type `C`. Unknown
/// fields, unknown commands, bad arguments and other protocol versions are
/// all errors.
pub fn decode_request<C: DeserializeOwned>(line: &str) -> Result<Request<C>, String> {
    read_request(line)?.decode()
}

/// Encodes a message as one line, newline included.
pub fn encode<T: Serialize>(body: T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&Envelope::new(body))?;
    line.push('\n');
    Ok(line)
}
