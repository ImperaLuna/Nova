use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod backoff;
pub mod reply_slot;
pub mod state_file;

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
