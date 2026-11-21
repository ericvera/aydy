//! The injectable clock every timing path reads.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A source of monotonic time.
///
/// Session timing reads the current instant through this trait rather than
/// calling [`Instant::now`] directly, so a test can move time by forty minutes
/// without waiting forty minutes.
pub trait Clock: Send + Sync {
    /// The current instant.
    fn now(&self) -> Instant;
}

/// The clock the running application uses.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock whose time moves only when [`TestClock::advance`] is called.
///
/// Shared by reference, so a test can hold the clock and hand a clone of the
/// same [`std::sync::Arc`] to the registry.
#[derive(Debug)]
pub struct TestClock {
    now: Mutex<Instant>,
}

impl TestClock {
    /// A clock starting at the current instant.
    pub fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
        }
    }

    /// Moves this clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().expect("the test clock mutex was poisoned");
        *now += by;
    }
}

impl Default for TestClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        *self.now.lock().expect("the test clock mutex was poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_clock_does_not_move_on_its_own() {
        let clock = TestClock::new();

        let first = clock.now();
        let second = clock.now();

        assert_eq!(first, second);
    }

    #[test]
    fn advancing_a_test_clock_moves_it_by_exactly_the_requested_amount() {
        let clock = TestClock::new();
        let before = clock.now();

        clock.advance(Duration::from_secs(90));

        assert_eq!(clock.now().duration_since(before), Duration::from_secs(90));
    }

    #[test]
    fn advances_accumulate() {
        let clock = TestClock::new();
        let before = clock.now();

        clock.advance(Duration::from_secs(60));
        clock.advance(Duration::from_secs(30));

        assert_eq!(clock.now().duration_since(before), Duration::from_secs(90));
    }

    #[test]
    fn a_test_clock_can_be_read_through_the_clock_trait() {
        let clock: Box<dyn Clock> = Box::new(TestClock::new());
        let before = clock.now();

        let after = clock.now();

        assert_eq!(before, after);
    }

    #[test]
    fn the_system_clock_never_moves_backwards() {
        let clock = SystemClock;

        let first = clock.now();
        let second = clock.now();

        assert!(second >= first);
    }
}
