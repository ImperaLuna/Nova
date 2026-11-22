//! Where a D-Bus call that waits for the user gets its answer: the call
//! awaits [`Slot::wait`], and whoever ends the question (the user's answer
//! over the socket, a cancel, the service stopping) calls
//! [`Slot::resolve`]. Only the first resolve counts. A thread can wait for
//! the reply too, with a deadline ([`Slot::wait_for`]), to end a question
//! nobody answers. Used by the Bluetooth pairing agent, Nova Files' file
//! picker portal, and recording (a recorder's exit).

use std::future::poll_fn;
use std::sync::{Condvar, Mutex, PoisonError};
use std::task::{Poll, Waker};
use std::time::Duration;

/// A reply of type `T`, once it's in.
pub struct Slot<T> {
    reply: Mutex<Option<T>>,
    /// Wakes the threads in [`Slot::wait_for`].
    resolved: Condvar,
    waker: Mutex<Option<Waker>>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self { reply: Mutex::new(None), resolved: Condvar::new(), waker: Mutex::new(None) }
    }
}

impl<T: Clone> Slot<T> {
    /// Ends the question with `reply`, unless it has ended already.
    pub fn resolve(&self, reply: T) {
        let mut slot = crate::lock(&self.reply);
        if slot.is_none() {
            *slot = Some(reply);
            drop(slot);
            self.resolved.notify_all();
            if let Some(waker) = crate::lock(&self.waker).take() {
                waker.wake();
            }
        }
    }

    /// The reply, blocking this thread until it's in or `timeout` passes
    /// (`None`).
    pub fn wait_for(&self, timeout: Duration) -> Option<T> {
        let reply = crate::lock(&self.reply);
        let (reply, _) = self
            .resolved
            .wait_timeout_while(reply, timeout, |reply| reply.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        reply.clone()
    }

    /// The reply, once one is in.
    pub async fn wait(&self) -> T {
        poll_fn(|cx| {
            // Stored before the check, so a reply in between still wakes us.
            *crate::lock(&self.waker) = Some(cx.waker().clone());
            match crate::lock(&self.reply).clone() {
                Some(reply) => Poll::Ready(reply),
                None => Poll::Pending,
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    const LONG: Duration = Duration::from_secs(30);

    #[test]
    fn a_waiting_thread_gets_the_reply_as_soon_as_it_is_in() {
        let slot = Arc::new(Slot::<u32>::default());
        let answering = Arc::clone(&slot);
        let answer = std::thread::spawn(move || answering.resolve(7));
        let started = Instant::now();
        assert_eq!(slot.wait_for(LONG), Some(7));
        assert!(started.elapsed() < LONG, "woken by the reply, not the deadline");
        answer.join().unwrap();
    }

    #[test]
    fn a_reply_already_in_returns_at_once() {
        let slot = Slot::default();
        slot.resolve("first");
        slot.resolve("second");
        assert_eq!(slot.wait_for(LONG), Some("first"), "only the first resolve counts");
    }

    #[test]
    fn no_reply_by_the_deadline_is_none() {
        let slot = Slot::<u32>::default();
        let timeout = Duration::from_millis(50);
        let started = Instant::now();
        assert_eq!(slot.wait_for(timeout), None);
        assert!(started.elapsed() >= timeout);
    }
}
