//! Locks the screen when logind asks, and before the machine sleeps (part of
//! the `lock` feature).
//!
//! On the system bus (systemd-logind, or elogind without systemd), novad
//! follows two of logind's signals:
//! - `Lock` on our session, which `loginctl lock-session`, idle daemons and
//!   lid handling send: novad locks the way `nova lock` does.
//! - `PrepareForSleep`, for a suspend from anywhere (the power menu, the lid,
//!   `systemctl suspend`). novad holds a `sleep` delay inhibitor, so logind
//!   waits with the suspend until novad lets go of it: once the lock screen
//!   covers the screens, or after `session_lock`'s cover timeout (logind's
//!   own `InhibitDelayMaxSec`, 5 s by default, may end the wait sooner). After
//!   the wake-up it takes a new one for the next sleep.
//!
//! Each has its own connection, retried on its own after a failure, so a
//! session novad can't find doesn't stop it locking before sleep.
//! `Unlock` isn't followed: only the user's password unlocks the screen.
//! The session is ours from `XDG_SESSION_ID` when it's set; a systemd user
//! service usually doesn't have it, so otherwise it's the session logind
//! counts as our user's display (the graphical one).

use crate::backoff::Backoff;
use crate::session_lock::FEATURE_ID;
use crate::{Daemon, lock};
use futures_core::Stream;
use std::future::poll_fn;
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::Duration;
use zbus::wire::{OwnedObjectPath, OwnedVariant};
use zbus::{Connection, MatchRule, Message, MessageStream, OwnedFd};

const LOG_SOURCE: &str = "logind";

pub const SERVICE: &str = "org.freedesktop.login1";
pub const PATH: &str = "/org/freedesktop/login1";
pub const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const USER: &str = "org.freedesktop.login1.User";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const SESSION_ID_VAR: &str = "XDG_SESSION_ID";
/// What `systemd-inhibit --list` shows for novad's inhibitor.
const INHIBITOR_WHO: &str = "Nova";
const INHIBITOR_WHY: &str = "Locking the screen before sleep";

/// How long to wait before following logind again after a failure,
/// doubling up to `MAX_RETRY` while it keeps failing.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);
/// How long connecting and each setup call get before novad starts over.
const SETUP_TIMEOUT: Duration = Duration::from_secs(5);

/// What novad follows logind for, each on its own connection and thread,
/// so one failing (no session to find) doesn't hold up the other.
#[derive(Clone, Copy)]
enum Follower {
    /// `PrepareForSleep`, with the sleep inhibitor.
    Sleep,
    /// `Lock` on our session.
    Lock,
}

/// Follows logind for as long as novad runs.
pub fn follow(daemon: &Arc<Daemon>) {
    for follower in [Follower::Sleep, Follower::Lock] {
        let daemon = Arc::clone(daemon);
        std::thread::spawn(move || keep_following(&daemon, follower));
    }
}

/// Follows logind for `follower`, starting over after a failure.
fn keep_following(daemon: &Daemon, follower: Follower) {
    let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
    let mut last_error = String::new();
    loop {
        let mut followed = false;
        let result = zbus::block_on(async {
            match follower {
                Follower::Sleep => follow_sleeps(daemon, &mut followed).await,
                Follower::Lock => follow_locks(daemon, &mut followed).await,
            }
        });
        if followed {
            retry.reset();
        }
        let wait = retry.next();
        let error = match result {
            Ok(()) => "logind's bus connection closed".to_string(),
            Err(e) => e.to_string(),
        };
        // A system without logind would otherwise log the same line every
        // minute.
        if error != last_error {
            let what = match follower {
                Follower::Sleep => "lock before sleep",
                Follower::Lock => "lock on `loginctl lock-session`",
            };
            let message = format!("can't {what}, trying again in {}s: {error}", wait.as_secs());
            daemon.logger.warn(LOG_SOURCE, &message);
            last_error = error;
        }
        std::thread::sleep(wait);
    }
}

/// Holds the sleep inhibitor and locks before each sleep, until the
/// connection closes (`Ok`) or fails. Sets `followed` once it holds the
/// inhibitor.
async fn follow_sleeps(daemon: &Daemon, followed: &mut bool) -> zbus::Result<()> {
    let (bus, mut sleeps, inhibitor) = within("connecting", async {
        let bus = Connection::system().await?;
        // Subscribed before the inhibitor is taken, so no sleep comes between.
        let sleeps = MessageStream::for_match_rule(signal(MANAGER, "PrepareForSleep", PATH)?, &bus, None).await?;
        let inhibitor = inhibit_sleep(&bus).await?;
        Ok((bus, sleeps, inhibitor))
    })
    .await?;
    let mut inhibitor = Some(inhibitor);
    *followed = true;
    while let Some(message) = next(&mut sleeps).await {
        let starting: bool = message?.body().deserialize()?;
        if starting {
            if let Some(inhibitor) = inhibitor.take() {
                release_once_locked(daemon, inhibitor);
            }
        } else if inhibitor.is_none() {
            inhibitor = Some(within("taking a new inhibitor", inhibit_sleep(&bus)).await?);
        }
    }
    Ok(())
}

/// Locks on each `Lock` logind sends our session, until the connection
/// closes (`Ok`) or fails. Sets `followed` once subscribed.
async fn follow_locks(daemon: &Daemon, followed: &mut bool) -> zbus::Result<()> {
    let (session, mut locks) = within("connecting", async {
        let bus = Connection::system().await?;
        let session = our_session(&bus).await?;
        let locks = MessageStream::for_match_rule(signal(SESSION, "Lock", session.as_str())?, &bus, None).await?;
        Ok((session, locks))
    })
    .await?;
    daemon.logger.info(LOG_SOURCE, &format!("following logind for session {}", session.as_str()));
    *followed = true;
    while let Some(message) = next(&mut locks).await {
        message?;
        lock_for(daemon, "logind asked");
    }
    Ok(())
}

/// The stream's next message, `None` once the connection closes.
async fn next(stream: &mut MessageStream) -> Option<zbus::Result<Message>> {
    poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

/// `step`'s result, or an error once `SETUP_TIMEOUT` has passed, so a
/// stalled bus or logind ends this connection and the retry starts over.
/// The wait for signals itself has no deadline.
async fn within<T>(step: &str, future: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    let expired = Arc::new(Deadline::default());
    let timer = Arc::clone(&expired);
    std::thread::spawn(move || {
        std::thread::sleep(SETUP_TIMEOUT);
        timer.passed.store(true, Ordering::SeqCst);
        if let Some(waker) = lock(&timer.waker).take() {
            waker.wake();
        }
    });
    let mut future = pin!(future);
    poll_fn(|cx| {
        if let Poll::Ready(result) = future.as_mut().poll(cx) {
            return Poll::Ready(result);
        }
        // Stored before the check, so a timer that fires in between still wakes us.
        *lock(&expired.waker) = Some(cx.waker().clone());
        match expired.passed.load(Ordering::SeqCst) {
            true => Poll::Ready(Err(zbus::Error::Failure(format!(
                "logind didn't answer within {} s while {step}",
                SETUP_TIMEOUT.as_secs()
            )))),
            false => Poll::Pending,
        }
    })
    .await
}

#[derive(Default)]
struct Deadline {
    passed: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// logind's signal `member` of `interface` from the object at `path`. Only
/// logind's own: anyone on the bus can send a signal.
fn signal<'m>(interface: &'m str, member: &'m str, path: &'m str) -> zbus::Result<MatchRule<'m>> {
    let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(SERVICE);
    rule.interface(interface).member(member).path(path).build()
}

/// Our session's object path.
async fn our_session(bus: &Connection) -> zbus::Result<OwnedObjectPath> {
    if let Some(id) = std::env::var(SESSION_ID_VAR).ok().filter(|id| !id.is_empty()) {
        let reply = bus.call_method(Some(SERVICE), PATH, Some(MANAGER), "GetSession", &(id,)).await?;
        return reply.body().deserialize();
    }
    // SAFETY: getuid can't fail and reads no memory.
    let uid = unsafe { libc::getuid() };
    let reply = bus.call_method(Some(SERVICE), PATH, Some(MANAGER), "GetUser", &(uid,)).await?;
    let user: OwnedObjectPath = reply.body().deserialize()?;
    let reply = bus.call_method(Some(SERVICE), user.as_str(), Some(PROPERTIES), "Get", &(USER, "Display")).await?;
    let (id, session): (String, OwnedObjectPath) = reply.body().deserialize::<OwnedVariant>()?.try_into()?;
    if id.is_empty() {
        return Err(zbus::Error::Failure("logind lists no session to lock for this user".into()));
    }
    Ok(session)
}

/// A `sleep` delay inhibitor, held until the file descriptor closes.
async fn inhibit_sleep(bus: &Connection) -> zbus::Result<OwnedFd> {
    let request = ("sleep", INHIBITOR_WHO, INHIBITOR_WHY, "delay");
    let reply: Message = bus.call_method(Some(SERVICE), PATH, Some(MANAGER), "Inhibit", &request).await?;
    reply.body().deserialize()
}

/// Locks the screen, when the lock feature is ready, and says whether a lock
/// was asked for. `why` goes to the log.
fn lock_for(daemon: &Daemon, why: &str) -> bool {
    if !daemon.feature_ready(FEATURE_ID) {
        return false;
    }
    daemon.logger.info(LOG_SOURCE, &format!("{why}: locking the screen"));
    match daemon.session_lock().request(&daemon.hub) {
        Ok(()) => true,
        Err(e) => {
            daemon.logger.warn(LOG_SOURCE, &format!("couldn't lock the screen: {e}"));
            false
        }
    }
}

/// Locks for a sleep that's starting, and lets it go on (closes `inhibitor`)
/// once the screens are covered. On its own thread, so logind's signals keep
/// being read meanwhile.
fn release_once_locked(daemon: &Daemon, inhibitor: OwnedFd) {
    if !lock_for(daemon, "the machine is going to sleep") {
        return drop(inhibitor);
    }
    let session_lock = daemon.session_lock();
    let logger = Arc::clone(&daemon.logger);
    std::thread::spawn(move || {
        if let Err(e) = session_lock.wait_covered() {
            logger.warn(LOG_SOURCE, &format!("sleeping though the screen may not be locked: {e}"));
        }
        drop(inhibitor);
    });
}
