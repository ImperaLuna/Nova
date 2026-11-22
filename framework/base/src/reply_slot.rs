//! Where a D-Bus call that waits for the user gets its answer: the call
//! awaits [`Slot::wait`], and whoever ends the question (the user's answer
//! over the socket, a cancel, the service stopping) calls
//! [`Slot::resolve`]. Only the first resolve counts. Used by the Bluetooth
//! pairing agent and Nova Files' file picker portal.

use std::future::poll_fn;
use std::sync::Mutex;
use std::task::{Poll, Waker};

/// A reply of type `T`, once it's in.
pub struct Slot<T> {
    reply: Mutex<Option<T>>,
    waker: Mutex<Option<Waker>>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self { reply: Mutex::new(None), waker: Mutex::new(None) }
    }
}

impl<T: Clone> Slot<T> {
    /// Ends the question with `reply`, unless it has ended already.
    pub fn resolve(&self, reply: T) {
        let mut slot = crate::lock(&self.reply);
        if slot.is_none() {
            *slot = Some(reply);
            drop(slot);
            if let Some(waker) = crate::lock(&self.waker).take() {
                waker.wake();
            }
        }
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
