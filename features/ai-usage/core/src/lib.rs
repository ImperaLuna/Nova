mod claude;
mod claude_api;
mod claude_desktop;
mod codex;
mod codex_api;
mod config;
mod endpoint;

pub use claude::limits_from_statusline;
pub use config::AiUsageConfig;

#[cfg(any(test, feature = "test-support"))]
pub use claude_desktop::{entry_name as claude_desktop_entry, write_unsealed_config as write_claude_desktop_config};

#[cfg(any(test, feature = "test-support"))]
pub fn claude_desktop_in(config_home: &Path) -> claude_desktop::Desktop {
    claude_desktop::Desktop::at(claude_desktop::config_in(config_home))
}

use codex::CodexUsage;
use endpoint::Problem;
use jiff::Timestamp;
use nova_ai_usage_api::{AiProvider, AiUsage, Event, ProviderUsage, UsageLimit, UsageState};
use nova_base::backoff::Backoff;
use nova_base::{lock, state_file};
use nova_hub::{ClientId, Hub};
use nova_inotify as inotify;
use nova_log::Logger;
use nova_modules::not_ready;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const FEATURE_ID: &str = "ai-usage";
const LOG_SOURCE: &str = "ai-usage";
const CODEX_SETTLE: Duration = Duration::from_millis(500);
const CODEX_MAX_DELAY: Duration = Duration::from_secs(5);
const IDLE_WAKE: Duration = Duration::from_secs(24 * 60 * 60);
const ENDPOINT_POLL: Duration = Duration::from_secs(5 * 60);
const ENDPOINT_REFRESH_GAP: Duration = Duration::from_secs(60);
const ENDPOINT_MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);
const ENDPOINT_RATE_LIMITED: Duration = Duration::from_secs(15 * 60);
const CLAUDE_ENDPOINT_OFF: &str = "Claude's limits show after a Claude Code session in a terminal. To see them anytime, add this to ~/.config/nova/config.toml:\n[ai-usage]\nclaude-usage-endpoint = true\nNova then asks Anthropic with Claude Code's sign-in, a use Anthropic doesn't document.";

#[derive(Clone)]
pub struct Paths {
    pub codex_home: Option<PathBuf>,
    pub state_file: PathBuf,
    pub claude_credentials: Option<PathBuf>,
    pub claude_desktop: Option<claude_desktop::Desktop>,
    pub claude_usage_url: String,
    pub codex_usage_url: String,
}

impl Paths {
    pub fn new(
        state_dir: &Path,
        codex_home: Option<PathBuf>,
        claude_credentials: Option<PathBuf>,
        claude_desktop_config: Option<PathBuf>,
    ) -> Self {
        Self {
            codex_home: codex_home.or_else(codex::default_home),
            state_file: state_dir.join("ai-usage.json"),
            claude_credentials: claude_credentials.or_else(claude_api::default_credentials),
            claude_desktop: claude_desktop_config
                .map(claude_desktop::Desktop::at)
                .or_else(claude_desktop::Desktop::default_location),
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
    refresh: Vec<Sender<()>>,
    stopped: AtomicBool,
    codex_watch: Mutex<Option<inotify::Stopper>>,
}

struct State {
    saved: Saved,
    claude_problem: Option<String>,
    claude_endpoint: bool,
    codex: Codex,
    claude_reported: Option<Instant>,
    sent: Option<AiUsage>,
}

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
    Changed,
    CodexLog(PathBuf),
    CodexRescan,
}

enum Endpoint {
    Claude { credentials: Option<PathBuf>, desktop: Option<claude_desktop::Desktop>, url: String },
    Codex { auth: Option<PathBuf>, url: String },
}

impl Endpoint {
    fn provider(&self) -> AiProvider {
        match self {
            Endpoint::Claude { .. } => AiProvider::Claude,
            Endpoint::Codex { .. } => AiProvider::Codex,
        }
    }

    fn ask(&self, stopped: &dyn Fn() -> bool) -> Result<(Vec<UsageLimit>, Option<String>), Problem> {
        let unless_stopped = || if stopped() { Err(Problem::Stopped) } else { Ok(()) };
        match self {
            Endpoint::Claude { credentials, desktop, url } => {
                let token = claude_token(credentials.as_deref(), desktop.as_ref(), stopped)?;
                unless_stopped()?;
                claude_api::fetch(url, &token).map(|limits| (limits, token.plan))
            }
            Endpoint::Codex { auth: Some(path), url } => {
                let token = codex_api::read_token(path)?;
                unless_stopped()?;
                codex_api::fetch(url, &token)
            }
            Endpoint::Codex { auth: None, .. } => Err(Problem::SignedOut),
        }
    }
}

fn claude_token(
    credentials: Option<&Path>,
    desktop: Option<&claude_desktop::Desktop>,
    stopped: &dyn Fn() -> bool,
) -> Result<claude_api::Token, Problem> {
    let now = Timestamp::now();
    let own = credentials.map_or(Err(Problem::SignedOut), |path| claude_api::read_token(path, now));
    match (own, desktop) {
        (Err(problem @ (Problem::SignedOut | Problem::Expired)), Some(desktop)) => {
            match desktop.read_token(now, stopped) {
                Err(Problem::SignedOut) => Err(problem),
                other => other,
            }
        }
        (own, _) => own,
    }
}

impl Usage {
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
            endpoints.push(Endpoint::Claude {
                credentials: paths.claude_credentials,
                desktop: paths.claude_desktop,
                url: paths.claude_usage_url,
            });
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
                claude_endpoint: config.claude_usage_endpoint,
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

    pub fn stop(&self) {
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

    pub fn event(&self) -> Event {
        Event::AiUsage(snapshot(&lock(&self.state), Timestamp::now()))
    }

    pub fn send_state(&self, client: ClientId) {
        self.hub.send_to(client, &self.event());
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

    pub fn refresh(&self) {
        for refresh in &self.refresh {
            let _ = refresh.send(());
        }
    }

    fn poll(&self, endpoint: &Endpoint, refreshes: mpsc::Receiver<()>) {
        let provider = endpoint.provider();
        let mut backoff = Backoff::new(ENDPOINT_POLL * 2, ENDPOINT_MAX_BACKOFF);
        let mut last_problem: Option<String> = None;
        loop {
            if self.stopped() {
                return;
            }
            let asked = Instant::now();
            let asked_at = Timestamp::now();
            let result = endpoint.ask(&|| self.stopped());
            let wait = match &result {
                Ok(_) | Err(Problem::SignedOut | Problem::Expired | Problem::Unsafe(_)) => {
                    backoff.reset();
                    ENDPOINT_POLL
                }
                Err(Problem::RateLimited(after)) => {
                    let wait = backoff.next_at_least(ENDPOINT_RATE_LIMITED);
                    after.map_or(wait, |after| after.max(wait)).min(ENDPOINT_MAX_BACKOFF)
                }
                Err(Problem::Failed(_)) => backoff.next(),
                Err(Problem::Stopped) => return,
            };
            let backing_off = wait > ENDPOINT_POLL;
            if self.stopped() {
                return;
            }
            match result {
                Ok((limits, plan)) => {
                    last_problem = None;
                    self.answered(provider, limits, plan, (asked, asked_at));
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

    fn answered(
        &self,
        provider: AiProvider,
        limits: Vec<UsageLimit>,
        plan: Option<String>,
        asked: (Instant, Timestamp),
    ) {
        let (asked, asked_at) = asked;
        match provider {
            AiProvider::Claude => {
                let _ = self.update(|state| {
                    state.claude_problem = None;
                    if state.claude_reported.is_none_or(|reported| reported < asked) {
                        state.saved.claude = Some(limits);
                        state.saved.claude_plan = plan;
                    }
                });
            }
            AiProvider::Codex => self.offer_codex(CodexUsage { plan, limits, logged_at: asked_at }),
        }
        let _ = self.wake.send(Wake::Changed);
    }

    fn update(&self, change: impl FnOnce(&mut State)) -> Result<(), String> {
        {
            let mut state = lock(&self.state);
            if self.stopped() {
                return Err(not_ready(FEATURE_ID));
            }
            let before = state.saved.clone();
            change(&mut state);
            if state.saved == before {
                return Ok(());
            }
            if let Err(e) = state_file::save(&self.state_file, &state.saved) {
                self.logger.warn(LOG_SOURCE, &format!("can't save {}: {e}", self.state_file.display()));
            }
        }
        let _ = self.wake.send(Wake::Changed);
        Ok(())
    }

    fn run(&self, codex_home: Option<PathBuf>, wakes: mpsc::Receiver<Wake>) {
        match &codex_home {
            Some(home) => {
                self.watch_codex(home);
                self.rescan_codex(home);
            }
            None => self.codex_unavailable("Can't find Codex's folder: HOME isn't set.".into()),
        }
        let mut written: BTreeSet<PathBuf> = BTreeSet::new();
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

    fn offer_codex(&self, found: CodexUsage) {
        let mut state = lock(&self.state);
        if let Codex::Found(current) = &state.codex
            && current.logged_at > found.logged_at
        {
            return;
        }
        state.codex = Codex::Found(found);
    }

    fn codex_unavailable(&self, hint: String) {
        let mut state = lock(&self.state);
        if !matches!(state.codex, Codex::Found(_)) {
            state.codex = Codex::Unavailable(hint);
        }
    }

    fn watch_codex(&self, home: &Path) {
        let home = std::path::absolute(home).unwrap_or_else(|_| home.to_owned());
        let sessions = codex::sessions_dir(&home);
        let watching = inotify::Inotify::new(inotify::WRITES).and_then(|mut watcher| {
            if let Some(parent) = home.parent() {
                watcher.watch_for(parent, inotify::ENTRIES)?;
            }
            watch_codex_home(&mut watcher, &home, &sessions)?;
            Ok(watcher)
        });
        let mut watcher = match watching {
            Ok(watcher) => watcher,
            Err(e) => {
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
        std::thread::spawn(move || follow_codex(&mut watcher, &CodexHome { home, sessions }, &wake, &logger));
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

struct CodexHome {
    home: PathBuf,
    sessions: PathBuf,
}

fn follow_codex(watcher: &mut inotify::Inotify, codex: &CodexHome, wake: &mpsc::Sender<Wake>, logger: &Logger) {
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
            rewatch_codex_home(watcher, codex, logger);
            if wake.send(Wake::CodexRescan).is_err() {
                return;
            }
            continue;
        }
        for change in changes.found {
            let Some(message) = codex_wake(watcher, codex, change, logger) else { continue };
            if wake.send(message).is_err() {
                return;
            }
        }
    }
}

fn codex_wake(
    watcher: &mut inotify::Inotify,
    codex: &CodexHome,
    change: inotify::Change,
    logger: &Logger,
) -> Option<Wake> {
    let CodexHome { home, sessions } = codex;
    if change.path == *home {
        rewatch_codex_home(watcher, codex, logger);
        Some(Wake::CodexRescan)
    } else if (change.is_dir && change.path.starts_with(sessions)) || change.path == *sessions {
        if let Err(e) = watcher.watch_tree(&change.path) {
            logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", change.path.display()));
        }
        Some(Wake::CodexRescan)
    } else if change.path.starts_with(sessions) && codex::is_log(&change.path) {
        Some(Wake::CodexLog(change.path))
    } else {
        None
    }
}

fn rewatch_codex_home(watcher: &mut inotify::Inotify, codex: &CodexHome, logger: &Logger) {
    if let Err(e) = watch_codex_home(watcher, &codex.home, &codex.sessions) {
        logger.warn(LOG_SOURCE, &format!("can't watch {}: {e}", codex.home.display()));
    }
}

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

fn snapshot(state: &State, now: Timestamp) -> AiUsage {
    let claude = match (&state.saved.claude, &state.claude_problem) {
        (Some(limits), Some(hint)) if state.claude_endpoint && all_reset(limits, now) => {
            unavailable(AiProvider::Claude, hint.clone())
        }
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

fn all_reset(limits: &[UsageLimit], now: Timestamp) -> bool {
    limits.iter().all(|limit| reset_time(limit).is_some_and(|at| at <= now))
}

fn loading(provider: AiProvider) -> ProviderUsage {
    ProviderUsage { provider, state: UsageState::Loading, plan: None, limits: vec![], hint: None }
}

fn unavailable(provider: AiProvider, hint: String) -> ProviderUsage {
    ProviderUsage { provider, state: UsageState::Unavailable, plan: None, limits: vec![], hint: Some(hint) }
}

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

fn display_plan(plan: &str) -> Option<String> {
    let mut chars = plan.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

fn round_percent(used: f64) -> u8 {
    used.round().clamp(0.0, 100.0) as u8
}
