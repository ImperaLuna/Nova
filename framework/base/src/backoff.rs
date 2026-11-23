use std::time::{Duration, Instant};

const FIRST_RESTART: Duration = Duration::from_secs(1);
const MAX_RESTART: Duration = Duration::from_secs(60);
const RAN_FINE: Duration = Duration::from_secs(30);

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
    #[cfg(any(test, feature = "test-support"))]
    pub const FIRST: Duration = FIRST_RESTART;

    pub fn started(&mut self) {
        self.started = Instant::now();
    }

    pub fn after_exit(&mut self) -> Duration {
        if self.started.elapsed() >= RAN_FINE {
            self.backoff.reset();
        }
        self.backoff.next()
    }
}

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
    pub const fn new(first: Duration, max: Duration) -> Self {
        Self { first, max, next: first }
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "one wait per try, never iterated; the name is what novad always called it"
    )]
    pub fn next(&mut self) -> Duration {
        self.next_at_least(Duration::ZERO)
    }

    pub fn next_at_least(&mut self, floor: Duration) -> Duration {
        let wait = self.next.max(floor).min(self.max);
        self.next = (wait * 2).min(self.max);
        wait
    }

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
