//! AI usage (feature `ai-usage`): how much of their Claude and Codex plan
//! limits the user has used. Claude's come from Claude Code's status line
//! (`nova claude-statusline`) when a terminal session runs and, if the user
//! turns it on, from Anthropic's usage endpoint with Claude Code's sign-in
//! (see `claude_api`), polled gently. Codex logs its limits in its session
//! files, which novad watches, after each turn; if the user turns it on,
//! OpenAI's usage endpoint is polled the same way (see `codex_api`), so the
//! numbers stay current between turns.
//!
//! Numbers change only when a source does, except at a window's reset time:
//! novad wakes then and shows that window at 0% until the tool is used again.

mod claude;
mod claude_api;
mod codex;
mod codex_api;
mod endpoint;

pub use claude::limits_from_statusline;

use crate::config::AiUsageConfig;
use crate::hub::Hub;
use crate::inotify;
use crate::lock;
use crate::logger::Logger;
use codex::CodexUsage;
use endpoint::Problem;
use jiff::Timestamp;
use nova_protocol::{AiProvider, AiUsage, Event, ProviderUsage, UsageLimit, UsageState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const FEATURE_ID: &str = "ai-usage";
const LOG_SOURCE: &str = "ai-usage";
/// Codex writes several lines per turn; they're read once it pauses.
const CODEX_SETTLE: Duration = Duration::from_millis(500);
/// The saved state is the user's own business.
const STATE_FILE_MODE: u32 = 0o600;
/// Sessions that never pause (several at once) are read this often anyway.
const CODEX_MAX_DELAY: Duration = Duration::from_secs(5);
/// Longest sleep with nothing scheduled.
const IDLE_WAKE: Duration = Duration::from_secs(24 * 60 * 60);
/// How often a usage endpoint is asked (nova-shell asked as often).
const ENDPOINT_POLL: Duration = Duration::from_secs(5 * 60);
/// Opening the panel asks again, but not more often than this.
const ENDPOINT_REFRESH_GAP: Duration = Duration::from_secs(60);
/// Failed requests wait twice as long each time, up to this.
const ENDPOINT_MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);
/// Being told to slow down waits at least this long.
const ENDPOINT_RATE_LIMITED: Duration = Duration::from_secs(15 * 60);
/// What the panel says while the endpoint is off and no status line has run.
const CLAUDE_ENDPOINT_OFF: &str = "Claude's limits show after a Claude Code session in a terminal. To see them anytime, add this to ~/.config/nova/config.toml:\n[ai-usage]\nclaude-usage-endpoint = true\nNova then asks Anthropic with Claude Code's sign-in, a use Anthropic doesn't document.";

/// Where the module reads and keeps things.
#[derive(Clone)]
pub struct Paths {
    /// Codex's home folder, `None` when neither `CODEX_HOME` nor `HOME` is set.
    pub codex_home: Option<PathBuf>,
    /// The bar's provider choice and Claude's last report.
    pub state_file: PathBuf,
    /// Claude Code's credentials file, `None` without `HOME`.
    pub claude_credentials: Option<PathBuf>,
    pub claude_usage_url: String,
    pub codex_usage_url: String,
}

impl Paths {
    pub fn new(state_dir: &Path, codex_home: Option<PathBuf>, claude_credentials: Option<PathBuf>) -> Self {
        Self {
            codex_home: codex_home.or_else(codex::default_home),
            state_file: state_dir.join("ai-usage.json"),
            claude_credentials: claude_credentials.or_else(claude_api::default_credentials),
            claude_usage_url: claude_api::USAGE_URL.into(),
            codex_usage_url: codex_api::USAGE_URL.into(),
        }
    }
}

pub struct Usage {
    state: Mutex<State>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    state_file: PathBuf,
    wake: Sender<Wake>,
    /// One per endpoint poller.
    refresh: Vec<Sender<()>>,
    /// Set by `stop`; every thread checks it when it wakes.
    stopped: AtomicBool,
    /// Ends the Codex watcher's thread; `None` until it starts.
    codex_watch: Mutex<Option<inotify::Stopper>>,
}

struct State {
    saved: Saved,
    /// Why there are no Claude numbers yet; `None` while the first answer
    /// is pending. Starts as the opt-in hint when the endpoint is off.
    claude_problem: Option<String>,
    codex: Codex,
    /// When the status line last reported Claude's limits. An endpoint
    /// answer to a request sent before then is older, so it's dropped.
    claude_reported: Option<Instant>,
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

/// A provider's usage endpoint, asked by `Usage::poll`.
enum Endpoint {
    Claude { credentials: Option<PathBuf>, url: String },
    Codex { auth: Option<PathBuf>, url: String },
}

impl Endpoint {
    fn provider(&self) -> AiProvider {
        match self {
            Endpoint::Claude { .. } => AiProvider::Claude,
            Endpoint::Codex { .. } => AiProvider::Codex,
        }
    }

    /// The limits and plan, read with the tool's sign-in.
    fn ask(&self) -> Result<(Vec<UsageLimit>, Option<String>), Problem> {
        match self {
            Endpoint::Claude { credentials: Some(path), url } => claude_api::read_token(path, Timestamp::now())
                .and_then(|token| claude_api::fetch(url, &token).map(|limits| (limits, token.plan))),
            Endpoint::Codex { auth: Some(path), url } => {
                codex_api::read_token(path).and_then(|token| codex_api::fetch(url, &token))
            }
            Endpoint::Claude { credentials: None, .. } | Endpoint::Codex { auth: None, .. } => Err(Problem::SignedOut),
        }
    }
}

impl Usage {
    /// Loads the saved state, starts watching Codex's logs and asking for
    /// Claude's limits.
    pub fn start(paths: Paths, config: &AiUsageConfig, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let saved = match std::fs::read_to_string(&paths.state_file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                logger.warn(LOG_SOURCE, &format!("ignoring {}: {e}", paths.state_file.display()));
                Saved::default()
            }),
            Err(_) => Saved::default(),
        };
        let (wake, wakes) = mpsc::channel();
        let mut endpoints = Vec::new();
        if config.claude_usage_endpoint {
            endpoints.push(Endpoint::Claude { credentials: paths.claude_credentials, url: paths.claude_usage_url });
        }
        if config.codex_usage_endpoint {
            let auth = paths.codex_home.as_deref().map(codex_api::auth_file);
            endpoints.push(Endpoint::Codex { auth, url: paths.codex_usage_url });
        }
        let (refresh, refreshes): (Vec<_>, Vec<_>) = endpoints.iter().map(|_| mpsc::channel()).unzip();
        let usage = Arc::new(Self {
            state: Mutex::new(State {
                saved,
                claude_problem: (!config.claude_usage_endpoint).then(|| CLAUDE_ENDPOINT_OFF.into()),
                codex: Codex::Loading,
                claude_reported: None,
                sent: None,
            }),
            hub,
            logger,
            state_file: paths.state_file,
            wake,
            refresh,
            stopped: AtomicBool::new(false),
            codex_watch: Mutex::new(None),
        });
        let worker = Arc::clone(&usage);
        std::thread::spawn(move || worker.run(paths.codex_home, wakes));
        for (endpoint, refreshes) in endpoints.into_iter().zip(refreshes) {
            let poller = Arc::clone(&usage);
            std::thread::spawn(move || poller.poll(&endpoint, refreshes));
        }
        usage
    }

    /// Ends its threads (watching, asking, publishing) and publishes nothing
    /// more. A request already on its way is dropped when it answers.
    pub fn stop(&self) {
        // Under the watcher's lock, so a watcher starting right now sees it,
        // and the state's, so nothing is published or saved after this.
        let mut codex_watch = lock(&self.codex_watch);
        {
            let _state = lock(&self.state);
            self.stopped.store(true, Ordering::SeqCst);
        }
        if let Some(watch) = codex_watch.take() {
            watch.stop();
        }
        let _ = self.wake.send(Wake::Changed);
        self.refresh();
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// The current numbers, as sent to a new subscriber. The broadcast
    /// bookkeeping (`sent`) is left alone: a change this already shows may
    /// still need publishing to everyone else.
    pub fn event(&self) -> Event {
        Event::AiUsage(snapshot(&lock(&self.state), Timestamp::now()))
    }

    pub fn show(&self, provider: AiProvider) -> Result<(), String> {
        self.update(|state| state.saved.shown = Some(provider))
    }

    pub fn report_claude(&self, limits: Vec<UsageLimit>) -> Result<(), String> {
        validate(&limits)?;
        self.update(|state| {
            state.saved.claude = Some(limits);
            state.claude_reported = Some(Instant::now());
        })
    }

    /// Asks the endpoints that are on now, unless one was asked less than
    /// `ENDPOINT_REFRESH_GAP` ago or is backing off.
    pub fn refresh(&self) {
        for refresh in &self.refresh {
            let _ = refresh.send(());
        }
    }

    /// Asks `endpoint` every `ENDPOINT_POLL`, and on `refresh`.
    fn poll(&self, endpoint: &Endpoint, refreshes: mpsc::Receiver<()>) {
        let provider = endpoint.provider();
        let mut backoff = ENDPOINT_POLL;
        let mut last_problem: Option<String> = None;
        loop {
            // Before reading the sign-in: a stopped poller asks nothing more.
            if self.stopped() {
                return;
            }
            let asked = Instant::now();
            let asked_at = Timestamp::now();
            let result = endpoint.ask();
            // Local problems cost the provider nothing; only failed requests back off.
            let wait = match &result {
                Ok(_) | Err(Problem::SignedOut | Problem::Expired | Problem::Unsafe(_)) => {
                    backoff = ENDPOINT_POLL;
                    ENDPOINT_POLL
                }
                Err(Problem::RateLimited(after)) => {
                    backoff = (backoff * 2).max(ENDPOINT_RATE_LIMITED).min(ENDPOINT_MAX_BACKOFF);
                    // Capped: a huge Retry-After would stop asking for good,
                    // or overflow the deadline below.
                    after.map_or(backoff, |after| after.max(backoff)).min(ENDPOINT_MAX_BACKOFF)
                }
                Err(Problem::Failed(_)) => {
                    backoff = (backoff * 2).min(ENDPOINT_MAX_BACKOFF);
                    backoff
                }
            };
            let backing_off = wait > ENDPOINT_POLL;
            if self.stopped() {
                return;
            }
            match result {
                Ok((limits, plan)) => {
                    last_problem = None;
                    match provider {
                        AiProvider::Claude => {
                            let _ = self.update(|state| {
                                state.claude_problem = None;
                                // The status line reported while this request was out: its numbers are newer.
                                if state.claude_reported.is_none_or(|reported| reported < asked) {
                                    state.saved.claude = Some(limits);
                                    state.saved.claude_plan = plan;
                                }
                            });
                        }
                        AiProvider::Codex => {
                            // As of the request: a log line written meanwhile is newer.
                            self.offer_codex(CodexUsage { plan, limits, logged_at: asked_at });
                            let _ = self.wake.send(Wake::Changed);
                        }
                    }
                }
                Err(problem) => {
                    let hint = problem.hint(provider);
                    if last_problem.as_ref() != Some(&hint) {
                        self.logger.info(LOG_SOURCE, &format!("{provider:?}'s limits: {hint}"));
                        last_problem = Some(hint.clone());
                    }
                    match provider {
                        AiProvider::Claude => lock(&self.state).claude_problem = Some(hint),
                        AiProvider::Codex => self.codex_unavailable(hint),
                    }
                    let _ = self.wake.send(Wake::Changed);
                }
            }
            let deadline = asked + wait;
            loop {
                match refreshes.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(()) if self.stopped() => return,
                    Ok(()) if !backing_off && asked.elapsed() >= ENDPOINT_REFRESH_GAP => break,
                    Ok(()) => {}
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }

    /// Applies `change` to the state, then saves and publishes what it
    /// changed of `Saved`.
    fn update(&self, change: impl FnOnce(&mut State)) -> Result<(), String> {
        {
            // Saved under the lock, so concurrent updates reach the file in
            // the order they were made.
            let mut state = lock(&self.state);
            if self.stopped() {
                return Err(format!("the {FEATURE_ID} feature is off"));
            }
            let before = state.saved.clone();
            change(&mut state);
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
            None => self.codex_unavailable("Can't find Codex's folder: HOME isn't set.".into()),
        }
        let mut written: BTreeSet<PathBuf> = BTreeSet::new();
        // When the oldest unread write in `written` came.
        let mut written_since: Option<Instant> = None;
        loop {
            if self.stopped() {
                return;
            }
            self.publish();
            let until_reset = next_reset(&lock(&self.state), Timestamp::now()).unwrap_or(IDLE_WAKE);
            let timeout = match written_since {
                None => until_reset,
                Some(since) => until_reset.min(CODEX_SETTLE).min(CODEX_MAX_DELAY.saturating_sub(since.elapsed())),
            };
            match wakes.recv_timeout(timeout) {
                Ok(Wake::CodexLog(path)) => {
                    written.insert(path);
                    let since = *written_since.get_or_insert_with(Instant::now);
                    if since.elapsed() < CODEX_MAX_DELAY {
                        continue;
                    }
                }
                Ok(Wake::CodexRescan) => {
                    if let Some(home) = &codex_home {
                        self.rescan_codex(home);
                    }
                }
                Ok(Wake::Changed) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            written_since = None;
            for path in std::mem::take(&mut written) {
                if let Some(found) = codex::read_log(&path) {
                    self.offer_codex(found);
                }
            }
        }
    }

    fn rescan_codex(&self, home: &Path) {
        let sessions = codex::sessions_dir(home);
        if !sessions.is_dir() {
            return self.codex_unavailable(format!("No Codex sessions in {} yet.", sessions.display()));
        }
        match codex::newest(&sessions) {
            Some(found) => self.offer_codex(found),
            None => {
                self.codex_unavailable("Codex hasn't logged its limits yet; they appear after its next reply.".into())
            }
        }
    }

    /// Takes limits from a log or the endpoint unless newer ones are
    /// already known.
    fn offer_codex(&self, found: CodexUsage) {
        let mut state = lock(&self.state);
        if let Codex::Found(current) = &state.codex
            && current.logged_at > found.logged_at
        {
            return;
        }
        state.codex = Codex::Found(found);
    }

    /// Says why there are no Codex numbers, unless there are some: the
    /// endpoint's, or a log's that has since gone, still beat none.
    fn codex_unavailable(&self, hint: String) {
        let mut state = lock(&self.state);
        if !matches!(state.codex, Codex::Found(_)) {
            state.codex = Codex::Unavailable(hint);
        }
    }

    /// Watches Codex's home for the sessions folder and every folder in it,
    /// on a thread that turns changes into wakes. The home itself is watched
    /// through its parent, so a home that appears later (before Codex's
    /// first run), is replaced, or is a symlink that's repointed is followed.
    fn watch_codex(&self, home: &Path) {
        // Absolute, so a relative home (`CODEX_HOME=codex`) has a parent to
        // watch and matches the paths changes come with.
        let home = std::path::absolute(home).unwrap_or_else(|_| home.to_owned());
        let sessions = codex::sessions_dir(&home);
        let watching = inotify::Inotify::new(inotify::WRITES).and_then(|mut watcher| {
            if let Some(parent) = home.parent() {
                watcher.watch_for(parent, inotify::ENTRIES)?;
            }
            // After the parent: a home that appears meanwhile is still seen.
            watch_codex_home(&mut watcher, &home, &sessions)?;
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
        {
            let mut codex_watch = lock(&self.codex_watch);
            if self.stopped() {
                return;
            }
            *codex_watch = Some(watcher.stopper());
        }
        let wake = self.wake.clone();
        let logger = Arc::clone(&self.logger);
        std::thread::spawn(move || {
            loop {
                let changes = match watcher.read() {
                    Ok(Some(changes)) => changes,
                    Ok(None) => return,
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("stopped watching Codex's logs: {e}"));
                        return;
                    }
                };
                if changes.overflowed {
                    // Some changes were dropped: watch everything again and rescan.
                    if let Err(e) = watch_codex_home(&mut watcher, &home, &sessions) {
                        logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", home.display()));
                    }
                    if wake.send(Wake::CodexRescan).is_err() {
                        return;
                    }
                    continue;
                }
                for change in changes.found {
                    // Any kind of entry: a symlinked home is repointed by
                    // renaming a new link over it.
                    let message = if change.path == home {
                        // The sessions folder may have come with it.
                        if let Err(e) = watch_codex_home(&mut watcher, &home, &sessions) {
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
        if self.stopped() {
            return;
        }
        let usage = snapshot(&state, Timestamp::now());
        if state.sent.as_ref() != Some(&usage) {
            self.hub.publish(&Event::AiUsage(usage.clone()));
            state.sent = Some(usage);
        }
    }
}

/// Watches Codex's home and every sessions folder, once they exist.
fn watch_codex_home(watcher: &mut inotify::Inotify, home: &Path, sessions: &Path) -> std::io::Result<()> {
    if !home.is_dir() {
        return Ok(());
    }
    watcher.watch(home)?;
    if sessions.is_dir() {
        watcher.watch_tree(sessions)?;
    }
    Ok(())
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

/// Writes a private temp file next to `path` and renames it over `path`.
fn save(path: &Path, saved: &Saved) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    // One left by a crash, or anything else in its place, goes first:
    // `create_new` never follows a link or reuses a file.
    match std::fs::remove_file(&partial) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(STATE_FILE_MODE).open(&partial)?;
    file.write_all(&serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?)?;
    std::fs::rename(partial, path)
}

/// A plan's display name: `plus` -> `Plus`; `None` when it's empty.
fn display_plan(plan: &str) -> Option<String> {
    let mut chars = plan.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// Whole percent, clamped to 0..=100.
fn round_percent(used: f64) -> u8 {
    used.round().clamp(0.0, 100.0) as u8
}
