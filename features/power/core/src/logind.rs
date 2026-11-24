use futures_core::Stream;
use nova_base::backoff::Backoff;
use nova_base::lock;
use nova_log::Logger;
use std::future::poll_fn;
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
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
const INHIBITOR_WHO: &str = "Nova";
const INHIBITOR_WHY: &str = "Locking the screen before sleep";

const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);
const SETUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Follower {
    Sleep,
    Lock,
}

pub trait LockScreen: Send + Sync + 'static {
    fn ready(&self) -> bool;
    fn request(&self) -> Result<(), String>;
    fn wait_covered(&self) -> Result<(), String>;
}

struct Locking {
    screen: Arc<dyn LockScreen>,
    logger: Arc<Logger>,
}

pub fn follow(screen: Arc<dyn LockScreen>, logger: Arc<Logger>) {
    let locking = Arc::new(Locking { screen, logger });
    for follower in [Follower::Sleep, Follower::Lock] {
        let locking = Arc::clone(&locking);
        std::thread::spawn(move || keep_following(&locking, follower));
    }
}

fn keep_following(locking: &Locking, follower: Follower) {
    let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
    let mut last_error = String::new();
    loop {
        let mut followed = false;
        let result = zbus::block_on(async {
            match follower {
                Follower::Sleep => follow_sleeps(locking, &mut followed).await,
                Follower::Lock => follow_locks(locking, &mut followed).await,
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
        if error != last_error {
            let what = match follower {
                Follower::Sleep => "lock before sleep",
                Follower::Lock => "lock on `loginctl lock-session`",
            };
            let message = format!("can't {what}, trying again in {}s: {error}", wait.as_secs());
            locking.logger.warn(LOG_SOURCE, &message);
            last_error = error;
        }
        nova_base::backoff::wait_out(wait);
    }
}

async fn follow_sleeps(locking: &Locking, followed: &mut bool) -> zbus::Result<()> {
    let (bus, mut sleeps, inhibitor) = within("connecting", async {
        let bus = Connection::system().await?;
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
                release_once_locked(locking, inhibitor);
            }
        } else if inhibitor.is_none() {
            inhibitor = Some(within("taking a new inhibitor", inhibit_sleep(&bus)).await?);
        }
    }
    Ok(())
}

async fn follow_locks(locking: &Locking, followed: &mut bool) -> zbus::Result<()> {
    let (session, mut locks) = within("connecting", async {
        let bus = Connection::system().await?;
        let session = our_session(&bus).await?;
        let locks = MessageStream::for_match_rule(signal(SESSION, "Lock", session.as_str())?, &bus, None).await?;
        Ok((session, locks))
    })
    .await?;
    locking.logger.info(LOG_SOURCE, &format!("following logind for session {}", session.as_str()));
    *followed = true;
    while let Some(message) = next(&mut locks).await {
        message?;
        lock_for(locking, "logind asked");
    }
    Ok(())
}

async fn next(stream: &mut MessageStream) -> Option<zbus::Result<Message>> {
    poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

async fn within<T>(step: &str, future: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    within_for(step, SETUP_TIMEOUT, future).await
}

async fn within_for<T>(
    step: &str,
    timeout: Duration,
    future: impl Future<Output = zbus::Result<T>>,
) -> zbus::Result<T> {
    let expired = Arc::new(Deadline::default());
    let timer = Arc::clone(&expired);
    let (_finished, done) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if done.recv_timeout(timeout) == Err(RecvTimeoutError::Timeout) {
            timer.passed.store(true, Ordering::SeqCst);
            if let Some(waker) = lock(&timer.waker).take() {
                waker.wake();
            }
        }
    });
    let mut future = pin!(future);
    poll_fn(|cx| {
        if let Poll::Ready(result) = future.as_mut().poll(cx) {
            return Poll::Ready(result);
        }
        *lock(&expired.waker) = Some(cx.waker().clone());
        match expired.passed.load(Ordering::SeqCst) {
            true => Poll::Ready(Err(zbus::Error::Failure(format!(
                "logind didn't answer within {} s while {step}",
                timeout.as_secs()
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

fn signal<'m>(interface: &'m str, member: &'m str, path: &'m str) -> zbus::Result<MatchRule<'m>> {
    let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(SERVICE);
    rule.interface(interface).member(member).path(path).build()
}

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

async fn inhibit_sleep(bus: &Connection) -> zbus::Result<OwnedFd> {
    let request = ("sleep", INHIBITOR_WHO, INHIBITOR_WHY, "delay");
    let reply: Message = bus.call_method(Some(SERVICE), PATH, Some(MANAGER), "Inhibit", &request).await?;
    reply.body().deserialize()
}

fn lock_for(locking: &Locking, why: &str) -> bool {
    if !locking.screen.ready() {
        return false;
    }
    locking.logger.info(LOG_SOURCE, &format!("{why}: locking the screen"));
    match locking.screen.request() {
        Ok(()) => true,
        Err(e) => {
            locking.logger.warn(LOG_SOURCE, &format!("couldn't lock the screen: {e}"));
            false
        }
    }
}

fn release_once_locked(locking: &Locking, inhibitor: OwnedFd) {
    if !lock_for(locking, "the machine is going to sleep") {
        return drop(inhibitor);
    }
    let session_lock = Arc::clone(&locking.screen);
    let logger = Arc::clone(&locking.logger);
    std::thread::spawn(move || {
        if let Err(e) = session_lock.wait_covered() {
            logger.warn(LOG_SOURCE, &format!("sleeping though the screen may not be locked: {e}"));
        }
        drop(inhibitor);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const LONG: Duration = Duration::from_secs(30);

    #[test]
    fn a_step_that_finishes_in_time_gives_its_result_at_once() {
        let started = Instant::now();
        let quick = async { Ok(7) };
        assert_eq!(zbus::block_on(within_for("testing", LONG, quick)).unwrap(), 7);
        assert!(started.elapsed() < LONG, "ended with the step, not the timeout");
    }

    #[test]
    fn a_step_that_never_finishes_fails_at_the_timeout() {
        let timeout = Duration::from_millis(50);
        let started = Instant::now();
        let stalled = std::future::pending::<zbus::Result<()>>();
        let error = zbus::block_on(within_for("connecting", timeout, stalled)).unwrap_err();
        assert!(error.to_string().contains("while connecting"), "{error}");
        assert!(started.elapsed() >= timeout);
    }
}
