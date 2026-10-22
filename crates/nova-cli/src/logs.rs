//! `nova logs`: novad's and the UI's log, live from novad or from its files.

use crate::client::Client;
use nova_protocol::{Command, Event, LogEntry, LogLevel, Topic, UI_LOG_SOURCE};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";

#[derive(clap::Args)]
pub struct LogsArgs {
    /// Lines of history to show first.
    #[arg(short = 'n', long, default_value_t = 50)]
    lines: usize,
    /// Print the history and exit instead of following new lines.
    #[arg(long)]
    no_follow: bool,
    /// Only the UI's lines.
    #[arg(long, conflicts_with = "daemon")]
    ui: bool,
    /// Only novad's lines.
    #[arg(long)]
    daemon: bool,
    /// Lowest level to show: debug, info, warn or error.
    #[arg(long)]
    level: Option<LogLevel>,
    /// The log of the run before the current one.
    #[arg(long, conflicts_with = "crash")]
    previous: bool,
    /// The lines saved at the latest crash.
    #[arg(long)]
    crash: bool,
    /// Where novad keeps its logs. By default, where the running novad
    /// says, else the usual place.
    #[arg(long, hide = true)]
    log_dir: Option<PathBuf>,
}

pub fn run(args: &LogsArgs, socket: &Path) -> Result<(), String> {
    let mut out = Printer { filter: Filter::from(args), color: std::io::stdout().is_terminal() };
    let log_dir = log_dir(args, socket);
    if args.crash {
        let crash = latest_crash(&log_dir)?;
        eprintln!("{}", crash.display());
        return out.file(&crash, usize::MAX);
    }
    if args.previous {
        return out.file(&nova_paths::previous_log_file(&log_dir, 1), args.lines);
    }
    if !args.no_follow
        && let Ok(client) = Client::connect(socket)
    {
        return out.follow(client, args.lines);
    }
    out.file(&nova_paths::log_file(&log_dir), args.lines)?;
    if !args.no_follow {
        eprintln!("novad isn't running, so this is the end of its last log. Start Nova with `nova`.");
    }
    Ok(())
}

/// `--log-dir`, else the running novad's (`just run` uses its own), else
/// the default.
fn log_dir(args: &LogsArgs, socket: &Path) -> PathBuf {
    args.log_dir.clone().unwrap_or_else(|| {
        Client::connect(socket)
            .and_then(|mut client| client.status())
            .map_or_else(|_| nova_paths::state_dir(), |status| PathBuf::from(status.log_dir))
    })
}

fn latest_crash(log_dir: &Path) -> Result<PathBuf, String> {
    let dir = nova_paths::crash_dir(log_dir);
    let names = std::fs::read_dir(&dir).map(|entries| entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>());
    names.unwrap_or_default().into_iter().max().ok_or_else(|| format!("no crashes saved in {}", dir.display()))
}

struct Filter {
    source: Option<Source>,
    min_level: LogLevel,
}

enum Source {
    Ui,
    Daemon,
}

impl From<&LogsArgs> for Filter {
    fn from(args: &LogsArgs) -> Self {
        let source = match (args.ui, args.daemon) {
            (true, _) => Some(Source::Ui),
            (_, true) => Some(Source::Daemon),
            _ => None,
        };
        Self { source, min_level: args.level.unwrap_or(LogLevel::Debug) }
    }
}

impl Filter {
    fn keeps(&self, entry: &LogEntry) -> bool {
        let source_ok = match self.source {
            None => true,
            Some(Source::Ui) => entry.source == UI_LOG_SOURCE,
            Some(Source::Daemon) => entry.source != UI_LOG_SOURCE,
        };
        source_ok && entry.level >= self.min_level
    }
}

struct Printer {
    filter: Filter,
    color: bool,
}

impl Printer {
    fn follow(&mut self, mut client: Client, history: usize) -> Result<(), String> {
        client.request(Command::Subscribe { topics: vec![Topic::Logs] })?;
        while let Some(event) = client.next_event()? {
            let Event::Logs { backlog, entries } = event else { continue };
            let kept: Vec<&LogEntry> = entries.iter().filter(|entry| self.filter.keeps(entry)).collect();
            let skip = if backlog { kept.len().saturating_sub(history) } else { 0 };
            for entry in &kept[skip..] {
                if !self.entry(entry) {
                    return Ok(());
                }
            }
        }
        eprintln!("novad stopped.");
        Ok(())
    }

    fn file(&mut self, path: &Path, history: usize) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let entries: Vec<LogEntry> =
            text.lines().filter_map(LogEntry::parse_line).filter(|entry| self.filter.keeps(entry)).collect();
        for entry in &entries[entries.len().saturating_sub(history)..] {
            if !self.entry(entry) {
                break;
            }
        }
        Ok(())
    }

    /// Prints one entry; false once stdout is closed (`nova logs | head`).
    fn entry(&mut self, entry: &LogEntry) -> bool {
        let time = local_time(&entry.time);
        let line = if self.color {
            let level = format!("{}{:>5}{RESET}", level_color(entry.level), entry.level);
            format!("{DIM}{time}{RESET} {level} {DIM}{}{RESET} {}", entry.source, entry.message)
        } else {
            format!("{time} {:>5} {} {}", entry.level, entry.source, entry.message)
        };
        writeln!(std::io::stdout(), "{line}").is_ok()
    }
}

fn level_color(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "\x1b[34m",
        LogLevel::Info => "\x1b[32m",
        LogLevel::Warn => "\x1b[33m",
        LogLevel::Error => "\x1b[31m",
    }
}

/// `13:45:09.123` in the local time zone, or the stored time if it won't parse.
fn local_time(time: &str) -> String {
    match time.parse::<jiff::Timestamp>() {
        Ok(timestamp) => timestamp.to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M:%S%.3f").to_string(),
        Err(_) => time.to_owned(),
    }
}
