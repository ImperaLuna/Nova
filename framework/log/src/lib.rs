use nova_base::lock;
use nova_carrier::log::{LogEntry, LogLevel, Logs};
use nova_hub::{ClientId, Hub};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const KEPT_RUNS: usize = 5;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const RECENT_ENTRIES: usize = 1000;
const CRASH_ENTRIES: usize = 300;
const KEPT_CRASHES: usize = 10;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const SECRET_KEYS: &[&str] = &["password", "passwd", "secret", "token", "api_key", "apikey"];
const REDACTED: &str = "<redacted>";

pub struct Logger {
    min_level: LogLevel,
    echo: bool,
    hub: Arc<Hub>,
    state: Mutex<State>,
}

struct State {
    dir: PathBuf,
    file: Option<File>,
    file_bytes: u64,
    recent: VecDeque<LogEntry>,
}

impl Logger {
    pub fn open(dir: &Path, min_level: LogLevel, echo: bool, hub: Arc<Hub>) -> std::io::Result<Self> {
        std::fs::DirBuilder::new().recursive(true).mode(PRIVATE_DIR_MODE).create(dir)?;
        let mut state = State { dir: dir.to_owned(), file: None, file_bytes: 0, recent: VecDeque::new() };
        state.start_new_file()?;
        Ok(Self { min_level, echo, hub, state: Mutex::new(state) })
    }

    pub fn dir(&self) -> PathBuf {
        lock(&self.state).dir.clone()
    }

    pub fn info(&self, source: &str, message: &str) {
        self.log(LogLevel::Info, source, message);
    }

    pub fn warn(&self, source: &str, message: &str) {
        self.log(LogLevel::Warn, source, message);
    }

    pub fn error(&self, source: &str, message: &str) {
        self.log(LogLevel::Error, source, message);
    }

    pub fn ui_line(&self, source: &str, line: &str) {
        let (level, message) = parse_ui_line(line);
        self.log(level, source, message);
    }

    pub fn log(&self, level: LogLevel, source: &str, message: &str) {
        if level < self.min_level {
            return;
        }
        let entries: Vec<LogEntry> = message
            .lines()
            .map(|line| LogEntry { time: now(), level, source: source.into(), message: redact(line) })
            .collect();
        let mut state = lock(&self.state);
        for entry in &entries {
            if self.echo {
                eprintln!("{}", entry.to_line());
            }
            state.write(entry);
        }
        // Published under the lock, so a subscriber never misses or repeats an entry between backlog and feed: https://linear.app/imperaluna/issue/IMP-8
        self.hub.publish(&Logs { backlog: false, entries }.event());
    }

    pub fn subscribe(&self, client: ClientId) {
        let state = lock(&self.state);
        let backlog = Logs { backlog: true, entries: state.recent.iter().cloned().collect() };
        self.hub.send_to(client, &backlog.event());
        self.hub.subscribe(client, nova_carrier::log::TOPIC);
    }

    pub fn save_crash(&self) -> std::io::Result<PathBuf> {
        lock(&self.state).save_crash()
    }

    pub fn try_save_crash(&self, message: &str) -> Option<PathBuf> {
        let mut state = self.state.try_lock().ok()?;
        state.write(&LogEntry {
            time: now(),
            level: LogLevel::Error,
            source: "novad".into(),
            message: redact(message),
        });
        state.save_crash().ok()
    }
}

impl State {
    fn write(&mut self, entry: &LogEntry) {
        if self.recent.len() == RECENT_ENTRIES {
            self.recent.pop_front();
        }
        self.recent.push_back(entry.clone());

        if self.file_bytes > MAX_FILE_BYTES
            && let Err(e) = self.start_new_file()
        {
            eprintln!("novad: can't rotate the log in {}: {e}", self.dir.display());
        }
        let Some(file) = &mut self.file else { return };
        let line = entry.to_line() + "\n";
        match file.write_all(line.as_bytes()) {
            Ok(()) => self.file_bytes += line.len() as u64,
            Err(e) => {
                eprintln!("novad: can't write the log, logging to it stops: {e}");
                self.file = None;
            }
        }
    }

    fn start_new_file(&mut self) -> std::io::Result<()> {
        self.file = None;
        for runs_ago in (1..KEPT_RUNS).rev() {
            rename_if_present(
                &nova_paths::previous_log_file(&self.dir, runs_ago),
                &nova_paths::previous_log_file(&self.dir, runs_ago + 1),
            )?;
        }
        rename_if_present(&nova_paths::log_file(&self.dir), &nova_paths::previous_log_file(&self.dir, 1))?;
        self.file = Some(private_file(&nova_paths::log_file(&self.dir))?);
        self.file_bytes = 0;
        Ok(())
    }

    fn save_crash(&mut self) -> std::io::Result<PathBuf> {
        let dir = nova_paths::crash_dir(&self.dir);
        std::fs::DirBuilder::new().recursive(true).mode(PRIVATE_DIR_MODE).create(&dir)?;
        let path = dir.join(format!("{}.log", now().replace(':', "-")));
        let mut file = private_file(&path)?;
        let skip = self.recent.len().saturating_sub(CRASH_ENTRIES);
        for entry in self.recent.iter().skip(skip) {
            writeln!(file, "{}", entry.to_line())?;
        }
        prune_crashes(&dir)?;
        Ok(path)
    }
}

fn private_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).write(true).truncate(true).mode(PRIVATE_FILE_MODE).open(path)
}

fn rename_if_present(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn prune_crashes(dir: &Path) -> std::io::Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|entry| entry.path()).collect();
    files.sort();
    let excess = files.len().saturating_sub(KEPT_CRASHES);
    for old in &files[..excess] {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

pub fn now() -> String {
    jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn parse_ui_line(line: &str) -> (LogLevel, &str) {
    let line = line.trim_end();
    let trimmed = line.trim_start();
    let (word, rest) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
    let level = match word.trim_end_matches(':') {
        "DEBUG" => LogLevel::Debug,
        "INFO" => LogLevel::Info,
        "WARN" => LogLevel::Warn,
        "ERROR" | "FATAL" => LogLevel::Error,
        _ => return (LogLevel::Info, line),
    };
    (level, rest.trim_start())
}

fn redact(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let mut out = String::with_capacity(message.len());
    let mut copied_to = 0;
    let mut search_from = 0;
    while let Some((key_end, value_start, value_end)) = next_secret(&lower, search_from) {
        out.push_str(&message[copied_to..value_start]);
        out.push_str(REDACTED);
        copied_to = value_end;
        search_from = value_end.max(key_end);
    }
    out.push_str(&message[copied_to..]);
    out
}

fn next_secret(lower: &str, from: usize) -> Option<(usize, usize, usize)> {
    let mut best: Option<(usize, usize, usize)> = None;
    for key in SECRET_KEYS {
        let mut start = from;
        while let Some(found) = lower[start..].find(key) {
            let key_end = start + found + key.len();
            start = key_end;
            let after_key = lower[key_end..].trim_start();
            let Some(after_separator) = after_key.strip_prefix(['=', ':']) else { continue };
            let value = after_separator.trim_start();
            let value_start = lower.len() - value.len();
            let value_end = value.find(char::is_whitespace).map_or(lower.len(), |end| value_start + end);
            if value_end > value_start && best.is_none_or(|(_, best_start, _)| value_start < best_start) {
                best = Some((key_end, value_start, value_end));
            }
            break;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_carrier::log::UI_LOG_SOURCE;

    fn temp_dir(name: &str) -> nova_test_support::TempDir {
        nova_test_support::TempDir::new(&format!("logger-{name}"))
    }

    fn logger(dir: &Path) -> Logger {
        Logger::open(dir, LogLevel::Info, false, Arc::new(Hub::default())).unwrap()
    }

    fn read_entries(path: &Path) -> Vec<LogEntry> {
        std::fs::read_to_string(path).unwrap().lines().map(|line| LogEntry::parse_line(line).unwrap()).collect()
    }

    #[test]
    fn each_start_rotates_and_keeps_five_runs() {
        let dir = temp_dir("rotate");
        for run in 0..8 {
            logger(&dir).info("novad", &format!("run {run}"));
        }
        assert_eq!(read_entries(&nova_paths::log_file(&dir))[0].message, "run 7");
        assert_eq!(read_entries(&nova_paths::previous_log_file(&dir, 1))[0].message, "run 6");
        assert_eq!(read_entries(&nova_paths::previous_log_file(&dir, KEPT_RUNS))[0].message, "run 2");
        assert!(!nova_paths::previous_log_file(&dir, KEPT_RUNS + 1).exists());
    }

    #[test]
    fn crash_snapshots_hold_the_latest_entries_and_keep_ten() {
        let dir = temp_dir("crash");
        let log = logger(&dir);
        for i in 0..CRASH_ENTRIES + 5 {
            log.ui_line(UI_LOG_SOURCE, &format!("  WARN line {i}"));
        }
        let snapshot = read_entries(&log.save_crash().unwrap());
        assert_eq!(snapshot.len(), CRASH_ENTRIES);
        assert_eq!(snapshot.last().unwrap().message, format!("line {}", CRASH_ENTRIES + 4));
        assert_eq!(snapshot[0].source, UI_LOG_SOURCE);

        for _ in 0..KEPT_CRASHES + 3 {
            log.save_crash().unwrap();
            nova_test_support::settle(std::time::Duration::from_millis(2));
        }
        assert_eq!(std::fs::read_dir(nova_paths::crash_dir(&dir)).unwrap().count(), KEPT_CRASHES);
    }

    #[test]
    fn entries_below_the_level_are_dropped_and_lines_split() {
        let dir = temp_dir("level");
        let log = Logger::open(&dir, LogLevel::Warn, false, Arc::new(Hub::default())).unwrap();
        log.info("novad", "quiet");
        log.error("novad", "first\nsecond");
        let messages: Vec<String> =
            read_entries(&nova_paths::log_file(&dir)).into_iter().map(|entry| entry.message).collect();
        assert_eq!(messages, ["first", "second"]);
    }

    #[test]
    fn quickshell_levels_are_read() {
        assert_eq!(parse_ui_line("  WARN qml: Binding loop"), (LogLevel::Warn, "qml: Binding loop"));
        assert_eq!(parse_ui_line(" ERROR: boom"), (LogLevel::Error, "boom"));
        assert_eq!(parse_ui_line(" FATAL quickshell: gone"), (LogLevel::Error, "quickshell: gone"));
        assert_eq!(parse_ui_line("  INFO: Launching config"), (LogLevel::Info, "Launching config"));
        assert_eq!(parse_ui_line("plain print\n"), (LogLevel::Info, "plain print"));
    }

    #[test]
    fn secrets_are_redacted() {
        assert_eq!(redact("login password=hunter2 ok"), "login password=<redacted> ok");
        assert_eq!(redact("Token: abc.def"), "Token: <redacted>");
        assert_eq!(redact("api_key = k1, secret:s2"), "api_key = <redacted> secret:<redacted>");
        assert_eq!(redact("no secrets, password manager open"), "no secrets, password manager open");
    }
}
