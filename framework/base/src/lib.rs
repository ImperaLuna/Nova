//! The smallest shared pieces novad's parts build on: locking a mutex that
//! a panicked thread held ([`lock`]), retry waits ([`backoff`]), the slot a
//! waiting D-Bus call gets its answer through ([`reply_slot`]), and private
//! files replaced in one step ([`state_file`]).

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod backoff;
pub mod reply_slot;
pub mod state_file;

/// Locks `mutex` even if another thread panicked while holding it: novad's
/// state stays usable, and the panic is already logged.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
