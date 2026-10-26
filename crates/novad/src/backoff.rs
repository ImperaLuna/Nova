//! A doubling retry wait, for the loops that start something again after it
//! failed (the UI, compositor IPC, logind, xembedsniproxy, AI usage
//! endpoints).

use std::time::Duration;

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
