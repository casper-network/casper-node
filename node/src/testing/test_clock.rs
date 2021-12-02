//! Testing clock
//!
//! A controllable clock for testing.
//!
//! # When to use `FakeClock` instead
//!
//! The [`TestClock`] is suitable for code written with "external" time passed in through its
//! regular interfaces already in mind. Code that does not conform to this should use `FakeClock`
//! and conditional compilation (`#[cfg(test)] ...`) instead.

use std::time::{Duration, Instant};

/// How far back the test clock can go (roughly 10 years).
const TEST_CLOCK_LEEWAY: Duration = Duration::from_secs(315_569_520);

/// A rewindable and forwardable clock for testing that does not tick on its own.
#[derive(Debug)]
pub struct TestClock {
    /// The current time set on the clock.
    now: Instant,
}

impl Default for TestClock {
    fn default() -> Self {
        TestClock::new()
    }
}

impl TestClock {
    /// Creates a new testing clock.
    ///
    /// Testing clocks will not advance unless prompted to do so.
    pub fn new() -> Self {
        Self {
            now: Instant::now() + TEST_CLOCK_LEEWAY,
        }
    }

    /// Returns the "current" time.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// Advances the clock by duration.
    pub fn advance(&mut self, duration: Duration) {
        self.now += duration;
    }

    /// Turns the clock by duration.
    pub fn rewind(&mut self, duration: Duration) {
        self.now -= duration;
    }

    /// `FakeClock` compatible interface.
    pub fn advance_time(&mut self, ms: u64) {
        self.advance(Duration::from_millis(ms))
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::TestClock;

    #[test]
    fn test_clock_operation() {
        let mut clock = TestClock::new();

        let initial = clock.now();

        // Ensure the clock does not advance on its own.
        thread::sleep(Duration::from_millis(10));

        assert_eq!(initial, clock.now());

        // Ensure the clock can go forwards and backwards.
        clock.advance(Duration::from_secs(1));
        clock.advance_time(1_000);

        assert_eq!(clock.now() - initial, Duration::from_secs(2));

        clock.rewind(Duration::from_secs(3));
        assert_eq!(initial - clock.now(), Duration::from_secs(1));
    }
}
