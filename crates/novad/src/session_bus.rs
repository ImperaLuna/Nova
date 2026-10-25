//! What novad's services on the session bus share: which bus to connect
//! to, and a stop flag that ends a service's loop on the bus.

use crate::lock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};
use zbus::connection;

/// Which bus a service is on.
#[derive(Clone)]
pub enum Bus {
    /// The session's, from `DBUS_SESSION_BUS_ADDRESS`.
    Session,
    /// A bus at this address, for tests.
    #[cfg(test)]
    Address(String),
}

impl Bus {
    pub fn builder(&self) -> connection::Builder<'static> {
        match self {
            Bus::Session => connection::Builder::session(),
            #[cfg(test)]
            Bus::Address(address) => connection::Builder::address(address.as_str()),
        }
    }
}

/// Ends a service's loop: set once, and wakes the loop if it's waiting.
#[derive(Default)]
pub struct Stop {
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Stop {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(waker) = lock(&self.waker).take() {
            waker.wake();
        }
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Ready once stopped; otherwise `cx` is woken by `stop`.
    pub fn poll(&self, cx: &Context<'_>) -> Poll<()> {
        // Stored before the check, so a stop in between still wakes us.
        *lock(&self.waker) = Some(cx.waker().clone());
        if self.stopped() { Poll::Ready(()) } else { Poll::Pending }
    }
}
