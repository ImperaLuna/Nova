use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 2;

pub const REQUEST_MAX_BYTES: u64 = 64 * 1024;

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
pub struct Request<C> {
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

#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    pub id: Option<u64>,
    pub cmd: String,
    pub args: Option<serde_json::Value>,
}

pub fn read_request(line: &str) -> Result<RawRequest, String> {
    let RequestLine { v, id, cmd, args } = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if v != PROTOCOL_VERSION {
        return Err(format!("protocol version {v} isn't supported; this daemon speaks {PROTOCOL_VERSION}"));
    }
    Ok(RawRequest { id, cmd, args })
}

impl RawRequest {
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

pub fn decode_request<C: DeserializeOwned>(line: &str) -> Result<Request<C>, String> {
    read_request(line)?.decode()
}

pub fn encode<T: Serialize>(body: T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&Envelope::new(body))?;
    line.push('\n');
    Ok(line)
}
