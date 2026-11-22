//! A doubling retry wait, for the loops that start something again after it
//! failed (the UI, compositor IPC, logind, AI usage endpoints), and
//! [`Restarts`], the same wait for the programs novad keeps running
//! (xembedsniproxy, awww, linux-wallpaperengine), and [`wait_out`], where
//! those loops wait.

use std::time::{Duration, Instant};

/// Waits before starting a program again: doubling from the first while it
/// keeps failing, back to the first once it ran this long.
const FIRST_RESTART: Duration = Duration::from_secs(1);
const MAX_RESTART: Duration = Duration::from_secs(60);
const RAN_FINE: Duration = Duration::from_secs(30);

/// When to start a long-running program again after it exits: soon after
/// a run that went fine for a while, longer each time it keeps failing.
#[derive(Debug, Clone)]
pub struct Restarts {
    backoff: Backoff,
    started: Instant,
}

impl Default for Restarts {
    fn default() -> Self {
        Self { backoff: Backoff::new(FIRST_RESTART, MAX_RESTART), started: Instant::now() }
    }
}

impl Restarts {
    /// The shortest wait, for tests that wait past one.
    #[cfg(any(test, feature = "test-support"))]
    pub const FIRST: Duration = FIRST_RESTART;

    /// The program was (re)started now.
    pub fn started(&mut self) {
        self.started = Instant::now();
    }

    /// How long to wait before starting it again, now that it exited.
    pub fn after_exit(&mut self) -> Duration {
        if self.started.elapsed() >= RAN_FINE {
            self.backoff.reset();
        }
        self.backoff.next()
    }
}

/// Waits out `time` before trying again something that sends no event to
/// wait on instead (a compositor or logind that isn't up, a program that
/// says it's ready only when asked). The one place novad waits on the
/// clock: a wait for something that can happen sooner waits on that thing,
/// with a timeout.
#[expect(clippy::disallowed_methods, reason = "retries against what sends no event wait here, and only here")]
pub fn wait_out(time: Duration) {
    std::thread::sleep(time);
}

#[derive(Debug, Clone)]
pub struct Backoff {
    first: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    /// Waits `first`, then twice as long each time, up to `max`.
    pub const fn new(first: Duration, max: Duration) -> Self {
        Self { first, max, next: first }
    }

    /// How long to wait before this try; the next wait doubles.
    #[expect(
        clippy::should_implement_trait,
        reason = "one wait per try, never iterated; the name is what novad always called it"
    )]
    pub fn next(&mut self) -> Duration {
        self.next_at_least(Duration::ZERO)
    }

    /// Like [`next`](Self::next), but at least `floor` (still capped at
    /// `max`), which the following waits double from.
    pub fn next_at_least(&mut self, floor: Duration) -> Duration {
        let wait = self.next.max(floor).min(self.max);
        self.next = (wait * 2).min(self.max);
        wait
    }

    /// Starts over from `first`, after something worked.
    pub fn reset(&mut self) {
        self.next = self.first;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_up_to_the_cap_and_starts_over() {
        let second = Duration::from_secs(1);
        let mut backoff = Backoff::new(second, second * 5);
        let waits: Vec<_> = (0..5).map(|_| backoff.next().as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 5, 5]);
        backoff.reset();
        assert_eq!(backoff.next(), second);
        assert_eq!(backoff.next_at_least(second * 3), second * 3);
        assert_eq!(backoff.next(), second * 5, "doubled from the floor, capped");
    }
}
