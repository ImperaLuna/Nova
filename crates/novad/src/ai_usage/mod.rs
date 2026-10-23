//! AI usage (feature `ai-usage`): how much of their Claude and Codex plan
//! limits the user has used. Claude's come from Claude Code's status line
//! (`nova claude-statusline`) when a terminal session runs and, if the user
//! turns it on, from Anthropic's usage endpoint with Claude Code's sign-in
//! (see `claude_api`), polled gently. Codex logs its limits in its session files, which novad
//! watches; nothing is requested for Codex.
//!
//! Numbers change only when a source does, except at a window's reset time:
//! novad wakes then and shows that window at 0% until the tool is used again.

mod claude;
mod claude_api;
mod codex;
mod inotify;

pub use claude::limits_from_statusline;

use crate::hub::Hub;
use crate::lock;
use crate::logger::Logger;
use codex::CodexUsage;
use jiff::Timestamp;
use nova_protocol::{AiProvider, AiUsage, Event, ProviderUsage, UsageLimit, UsageState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const FEATURE_ID: &str = "ai-usage";
const LOG_SOURCE: &str = "ai-usage";
/// Codex writes several lines per turn; they're read once it pauses.
const CODEX_SETTLE: Duration = Duration::from_millis(500);
/// Longest sleep with nothing scheduled.
const IDLE_WAKE: Duration = Duration::from_secs(24 * 60 * 60);
/// How often Claude's limits are asked for (nova-shell asked as often).
const CLAUDE_POLL: Duration = Duration::from_secs(5 * 60);
/// Opening the panel asks again, but not more often than this.
const CLAUDE_REFRESH_GAP: Duration = Duration::from_secs(60);
/// Failed requests wait twice as long each time, up to this.
const CLAUDE_MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);
/// Being told to slow down waits at least this long.
const CLAUDE_RATE_LIMITED: Duration = Duration::from_secs(15 * 60);
/// What the panel says while the endpoint is off and no status line has run.
const CLAUDE_ENDPOINT_OFF: &str = "Claude's limits show after a Claude Code session in a terminal. To see them anytime, add this to ~/.config/nova/config.toml:\n[ai-usage]\nclaude-usage-endpoint = true\nNova then asks Anthropic with Claude Code's sign-in, a use Anthropic doesn't document.";

/// Where the module reads and keeps things.
pub struct Paths {
    /// Codex's home folder, `None` when neither `CODEX_HOME` nor `HOME` is set.
    pub codex_home: Option<PathBuf>,
    /// The bar's provider choice and Claude's last report.
    pub state_file: PathBuf,
    /// Claude Code's credentials file, `None` without `HOME`.
    pub claude_credentials: Option<PathBuf>,
    pub claude_usage_url: String,
    /// Whether the user turned the usage endpoint on.
    pub claude_usage_endpoint: bool,
}

impl Paths {
    pub fn new(
        state_dir: &Path,
        codex_home: Option<PathBuf>,
        claude_credentials: Option<PathBuf>,
        claude_usage_endpoint: bool,
    ) -> Self {
        Self {
            codex_home: codex_home.or_else(codex::default_home),
            state_file: state_dir.join("ai-usage.json"),
            claude_credentials: claude_credentials.or_else(claude_api::default_credentials),
            claude_usage_url: claude_api::USAGE_URL.into(),
            claude_usage_endpoint,
        }
    }
}

pub struct Usage {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    state_file: PathBuf,
    wake: Sender<Wake>,
    refresh: Sender<()>,
}

struct State {
    saved: Saved,
    /// Why there are no Claude numbers yet; `None` while the first answer
    /// is pending. Starts as the opt-in hint when the endpoint is off.
    claude_problem: Option<String>,
    codex: Codex,
    /// The last event published, so unchanged numbers aren't sent again.
    sent: Option<AiUsage>,
}

/// What survives a restart.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    shown: Option<AiProvider>,
    #[serde(default)]
    claude: Option<Vec<UsageLimit>>,
    #[serde(default)]
    claude_plan: Option<String>,
}

enum Codex {
    Loading,
    Unavailable(String),
    Found(CodexUsage),
}

enum Wake {
    /// State changed; publish and recompute the next reset.
    Changed,
    /// A Codex log was written to.
    CodexLog(PathBuf),
    /// The sessions folder appeared or moved; look again from scratch.
    CodexRescan,
}

impl Usage {
    /// Loads the saved state, starts watching Codex's logs and asking for
    /// Claude's limits.
    pub fn start(paths: Paths, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let saved = match std::fs::read_to_string(&paths.state_file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", paths.state_file.display()));
                Saved::default()
            }),
            Err(_) => Saved::default(),
        };
        let (wake, wakes) = mpsc::channel();
        let (refresh, refreshes) = mpsc::channel();
        let usage = Arc::new(Self {
            state: Mutex::new(State {
                saved,
                claude_problem: (!paths.claude_usage_endpoint).then(|| CLAUDE_ENDPOINT_OFF.into()),
                codex: Codex::Loading,
                sent: None,
            }),
            hub,
            logger,
            state_file: paths.state_file,
            wake,
            refresh,
        });
        let worker = Arc::clone(&usage);
        std::thread::spawn(move || worker.run(paths.codex_home, wakes));
        if paths.claude_usage_endpoint {
            let poller = Arc::clone(&usage);
            let claude = (paths.claude_credentials, paths.claude_usage_url);
            std::thread::spawn(move || poller.poll_claude(claude.0, &claude.1, refreshes));
        }
        usage
    }

    /// The current numbers, as sent to a new subscriber. The broadcast
    /// bookkeeping (`sent`) is left alone: a change this already shows may
    /// still need publishing to everyone else.
    pub fn event(&self) -> Event {
        Event::AiUsage(snapshot(&lock(&self.state), Timestamp::now()))
    }

    pub fn show(&self, provider: AiProvider) -> Result<(), String> {
        self.update(|saved| saved.shown = Some(provider))
    }

    pub fn report_claude(&self, limits: Vec<UsageLimit>) -> Result<(), String> {
        validate(&limits)?;
        self.update(|saved| saved.claude = Some(limits))
    }

    /// Asks for Claude's limits now, unless they were asked for less than
    /// `CLAUDE_REFRESH_GAP` ago or requests are backing off.
    pub fn refresh(&self) {
        let _ = self.refresh.send(());
    }

    /// Asks for Claude's limits every `CLAUDE_POLL`, and on `refresh`.
    fn poll_claude(&self, credentials: Option<PathBuf>, url: &str, refreshes: mpsc::Receiver<()>) {
        let mut backoff = CLAUDE_POLL;
        let mut last_problem: Option<String> = None;
        loop {
            let asked = Instant::now();
            let result = match &credentials {
                Some(path) => claude_api::read_token(path, Timestamp::now())
                    .and_then(|token| claude_api::fetch(url, &token).map(|limits| (limits, token.plan))),
                None => Err(claude_api::Problem::SignedOut),
            };
            // Local problems cost Anthropic nothing; only failed requests back off.
            let wait = match &result {
                Ok(_)
                | Err(claude_api::Problem::SignedOut | claude_api::Problem::Expired | claude_api::Problem::Unsafe(_)) =>
                {
                    backoff = CLAUDE_POLL;
                    CLAUDE_POLL
                }
                Err(claude_api::Problem::RateLimited(after)) => {
                    backoff = (backoff * 2).max(CLAUDE_RATE_LIMITED).min(CLAUDE_MAX_BACKOFF);
                    after.map_or(backoff, |after| after.max(backoff))
                }
                Err(claude_api::Problem::Failed(_)) => {
                    backoff = (backoff * 2).min(CLAUDE_MAX_BACKOFF);
                    backoff
                }
            };
            let backing_off = wait > CLAUDE_POLL;
            match result {
                Ok((limits, plan)) => {
                    last_problem = None;
                    lock(&self.state).claude_problem = None;
                    let _ = self.update(|saved| {
                        saved.claude = Some(limits);
                        saved.claude_plan = plan;
                    });
                }
                Err(problem) => {
                    let hint = problem.hint();
                    if last_problem.as_ref() != Some(&hint) {
                        self.logger.info(LOG_SOURCE, &format!("Claude's limits: {hint}"));
                        last_problem = Some(hint.clone());
                    }
                    lock(&self.state).claude_problem = Some(hint);
                    let _ = self.wake.send(Wake::Changed);
                }
            }
            let deadline = asked + wait;
            loop {
                match refreshes.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(()) if !backing_off && asked.elapsed() >= CLAUDE_REFRESH_GAP => break,
                    Ok(()) => {}
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }

    fn update(&self, change: impl FnOnce(&mut Saved)) -> Result<(), String> {
        {
            // Saved under the lock, so concurrent updates reach the file in
            // the order they were made.
            let mut state = lock(&self.state);
            let before = state.saved.clone();
            change(&mut state.saved);
            if state.saved == before {
                return Ok(());
            }
            if let Err(e) = save(&self.state_file, &state.saved) {
                self.logger.warn(LOG_SOURCE, &format!("can't save {}: {e}", self.state_file.display()));
            }
        }
        let _ = self.wake.send(Wake::Changed);
        Ok(())
    }

    fn run(&self, codex_home: Option<PathBuf>, wakes: mpsc::Receiver<Wake>) {
        match &codex_home {
            Some(home) => {
                // Watched first, so nothing written between the two is missed.
                self.watch_codex(home);
                self.rescan_codex(home);
            }
            None => self.set_codex(Codex::Unavailable("Can't find Codex's folder: HOME isn't set.".into())),
        }
        let mut written: BTreeSet<PathBuf> = BTreeSet::new();
        loop {
            self.publish();
            let until_reset = next_reset(&lock(&self.state), Timestamp::now()).unwrap_or(IDLE_WAKE);
            let timeout = if written.is_empty() { until_reset } else { until_reset.min(CODEX_SETTLE) };
            match wakes.recv_timeout(timeout) {
                Ok(Wake::CodexLog(path)) => {
                    written.insert(path);
                    continue;
                }
                Ok(Wake::CodexRescan) => {
                    if let Some(home) = &codex_home {
                        self.rescan_codex(home);
                    }
                }
                Ok(Wake::Changed) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            for path in std::mem::take(&mut written) {
                if let Some(found) = codex::read_log(&path) {
                    self.offer_codex(found);
                }
            }
        }
    }

    fn rescan_codex(&self, home: &Path) {
        let sessions = codex::sessions_dir(home);
        let codex = if !sessions.is_dir() {
            Codex::Unavailable(format!("No Codex sessions in {} yet.", sessions.display()))
        } else {
            match codex::newest(&sessions) {
                Some(found) => return self.offer_codex(found),
                None => {
                    Codex::Unavailable("Codex hasn't logged its limits yet; they appear after its next reply.".into())
                }
            }
        };
        self.set_codex(codex);
    }

    /// Takes limits from a log unless newer ones are already known.
    fn offer_codex(&self, found: CodexUsage) {
        let mut state = lock(&self.state);
        if let Codex::Found(current) = &state.codex
            && current.logged_at > found.logged_at
        {
            return;
        }
        state.codex = Codex::Found(found);
    }

    fn set_codex(&self, codex: Codex) {
        lock(&self.state).codex = codex;
    }

    /// Watches Codex's home for the sessions folder and every folder in it,
    /// on a thread that turns changes into wakes. Before Codex's first run
    /// there's no home yet, so its parent is watched for it to appear.
    fn watch_codex(&self, home: &Path) {
        let home = home.to_owned();
        let sessions = codex::sessions_dir(&home);
        let watching = inotify::Inotify::new().and_then(|mut watcher| {
            if !home.is_dir()
                && let Some(parent) = home.parent()
            {
                watcher.watch(parent)?;
            }
            // Checked again: the home may have appeared meanwhile.
            if home.is_dir() {
                watcher.watch(&home)?;
                if sessions.is_dir() {
                    watcher.watch_tree(&sessions)?;
                }
            }
            Ok(watcher)
        });
        let mut watcher = match watching {
            Ok(watcher) => watcher,
            Err(e) => {
                // Without a folder there is nothing to watch; the hint says so.
                if home.is_dir() {
                    self.logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", home.display()));
                }
                return;
            }
        };
        let wake = self.wake.clone();
        let logger = Arc::clone(&self.logger);
        std::thread::spawn(move || {
            loop {
                let changes = match watcher.read() {
                    Ok(changes) => changes,
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("stopped watching Codex's logs: {e}"));
                        return;
                    }
                };
                for change in changes {
                    let message = if change.is_dir && change.path == home {
                        // The sessions folder may have come with it.
                        let watched = watcher
                            .watch(&home)
                            .and_then(|()| if sessions.is_dir() { watcher.watch_tree(&sessions) } else { Ok(()) });
                        if let Err(e) = watched {
                            logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", home.display()));
                        }
                        Wake::CodexRescan
                    } else if (change.is_dir && change.path.starts_with(&sessions)) || change.path == sessions {
                        if let Err(e) = watcher.watch_tree(&change.path) {
                            logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", change.path.display()));
                        }
                        Wake::CodexRescan
                    } else if change.path.starts_with(&sessions) && codex::is_log(&change.path) {
                        Wake::CodexLog(change.path)
                    } else {
                        continue;
                    };
                    if wake.send(message).is_err() {
                        return;
                    }
                }
            }
        });
    }

    fn publish(&self) {
        let mut state = lock(&self.state);
        let usage = snapshot(&state, Timestamp::now());
        if state.sent.as_ref() != Some(&usage) {
            self.hub.publish(&Event::AiUsage(usage.clone()));
            state.sent = Some(usage);
        }
    }
}

/// The event for `state` at `now`: windows past their reset show 0%.
fn snapshot(state: &State, now: Timestamp) -> AiUsage {
    let claude = match (&state.saved.claude, &state.claude_problem) {
        (Some(limits), _) => ready(AiProvider::Claude, state.saved.claude_plan.clone(), limits, now),
        (None, Some(hint)) => unavailable(AiProvider::Claude, hint.clone()),
        (None, None) => loading(AiProvider::Claude),
    };
    let codex = match &state.codex {
        Codex::Loading => loading(AiProvider::Codex),
        Codex::Unavailable(hint) => unavailable(AiProvider::Codex, hint.clone()),
        Codex::Found(found) => ready(AiProvider::Codex, found.plan.clone(), &found.limits, now),
    };
    AiUsage { shown: state.saved.shown.unwrap_or(AiProvider::Claude), providers: vec![claude, codex] }
}

fn ready(provider: AiProvider, plan: Option<String>, limits: &[UsageLimit], now: Timestamp) -> ProviderUsage {
    let limits = limits
        .iter()
        .map(|limit| match reset_time(limit) {
            Some(at) if at <= now => UsageLimit { window: limit.window, percent: 0, resets_at: None },
            _ => limit.clone(),
        })
        .collect();
    ProviderUsage { provider, state: UsageState::Ready, plan, limits, hint: None }
}

fn loading(provider: AiProvider) -> ProviderUsage {
    ProviderUsage { provider, state: UsageState::Loading, plan: None, limits: vec![], hint: None }
}

fn unavailable(provider: AiProvider, hint: String) -> ProviderUsage {
    ProviderUsage { provider, state: UsageState::Unavailable, plan: None, limits: vec![], hint: Some(hint) }
}

/// How long until the next window resets.
fn next_reset(state: &State, now: Timestamp) -> Option<Duration> {
    let codex = match &state.codex {
        Codex::Found(found) => found.limits.as_slice(),
        _ => &[],
    };
    let claude = state.saved.claude.as_deref().unwrap_or_default();
    claude
        .iter()
        .chain(codex)
        .filter_map(reset_time)
        .filter(|at| *at > now)
        .min()
        .and_then(|at| Duration::try_from(at.duration_since(now)).ok())
}

fn reset_time(limit: &UsageLimit) -> Option<Timestamp> {
    limit.resets_at.as_deref()?.parse().ok()
}

/// Reports come over the socket, so they're checked like any request.
fn validate(limits: &[UsageLimit]) -> Result<(), String> {
    for (index, limit) in limits.iter().enumerate() {
        if limit.percent > 100 {
            return Err(format!("percent {} is over 100", limit.percent));
        }
        if limits[..index].iter().any(|earlier| earlier.window == limit.window) {
            return Err(format!("{:?} limit given twice", limit.window));
        }
        if let Some(at) = &limit.resets_at
            && at.parse::<Timestamp>().is_err()
        {
            return Err(format!("resets_at {at:?} isn't an RFC 3339 time"));
        }
    }
    Ok(())
}

fn save(path: &Path, saved: &Saved) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?)?;
    std::fs::rename(partial, path)
}

/// Whole percent, clamped to 0..=100.
fn round_percent(used: f64) -> u8 {
    used.round().clamp(0.0, 100.0) as u8
}
