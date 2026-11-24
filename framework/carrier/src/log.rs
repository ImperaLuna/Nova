use crate::topic::{Single, Topic};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub const TOPIC: Topic = Topic::named("logs");
pub const EVENT: &str = "logs";

pub const UI_LOG_SOURCE: &str = "ui";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logs {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backlog: bool,
    pub entries: Vec<LogEntry>,
}

impl Logs {
    pub fn event(&self) -> Single<'_, Self> {
        Single::new(EVENT, TOPIC, self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub time: String,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
}

impl LogEntry {
    pub fn to_line(&self) -> String {
        format!("{} {:>5} {}: {}", self.time, self.level, self.source, self.message)
    }

    pub fn parse_line(line: &str) -> Option<Self> {
        let (time, rest) = line.split_once(' ')?;
        let rest = rest.trim_start();
        let (level, rest) = rest.split_once(' ')?;
        let (source, message) = rest.split_once(": ")?;
        Some(Self { time: time.into(), level: level.parse().ok()?, source: source.into(), message: message.into() })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const ALL: [LogLevel; 4] = [LogLevel::Debug, LogLevel::Info, LogLevel::Warn, LogLevel::Error];

    pub fn name(self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| level.name().eq_ignore_ascii_case(text))
            .ok_or_else(|| format!("unknown log level {text:?} (known: debug, info, warn, error)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_entry() -> LogEntry {
        LogEntry {
            time: "2026-10-22T13:45:09.123Z".into(),
            level: LogLevel::Warn,
            source: UI_LOG_SOURCE.into(),
            message: "Binding loop detected: x: y".into(),
        }
    }

    #[test]
    fn log_lines_round_trip() {
        let entry = log_entry();
        let line = entry.to_line();
        assert_eq!(line, "2026-10-22T13:45:09.123Z  WARN ui: Binding loop detected: x: y");
        assert_eq!(LogEntry::parse_line(&line), Some(entry));
        assert_eq!(LogEntry::parse_line("not a log line"), None);
    }

    #[test]
    fn log_levels_parse_any_case_and_order_by_severity() {
        assert_eq!("WARN".parse::<LogLevel>(), Ok(LogLevel::Warn));
        assert_eq!("error".parse::<LogLevel>(), Ok(LogLevel::Error));
        assert!("loud".parse::<LogLevel>().is_err());
        assert!(LogLevel::Debug < LogLevel::Info && LogLevel::Warn < LogLevel::Error);
    }
}
