//! Working-time and longest-turn accounting.

use std::time::{Duration, Instant};

/// The two durations tracked per session.
///
/// They are measured differently on purpose. **Working time** accumulates only
/// while the session is Working, so a mid-turn wait is excluded from it.
/// **Longest turn** is elapsed time from prompt submission to turn completion,
/// so the same wait is included in it. A turn that worked 1 minute, waited 10
/// and worked 39 more reports 40 minutes of working time and a 50-minute turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionTiming {
    working_total: Duration,
    working_since: Option<Instant>,
    turn_started_at: Option<Instant>,
    longest_turn: Duration,
}

impl SessionTiming {
    /// Starts or stops the working clock so it matches `working`.
    ///
    /// Called after every change to a session's facts, with the status derived
    /// from those facts, so the clock cannot drift from the status.
    pub fn set_working(&mut self, working: bool, now: Instant) {
        match (working, self.working_since) {
            (true, None) => self.working_since = Some(now),
            (false, Some(since)) => {
                self.working_total += now.saturating_duration_since(since);
                self.working_since = None;
            }
            _ => {}
        }
    }

    /// Opens a turn for the longest-turn measurement.
    ///
    /// A prompt submitted while a turn is already open does not restart the
    /// measurement: the turn began at the first prompt and has not completed.
    pub fn start_turn(&mut self, now: Instant) {
        if self.turn_started_at.is_none() {
            self.turn_started_at = Some(now);
        }
    }

    /// Completes the open turn, recording it if it is the longest so far.
    pub fn end_turn(&mut self, now: Instant) {
        if let Some(started_at) = self.turn_started_at.take() {
            let elapsed = now.saturating_duration_since(started_at);
            if elapsed > self.longest_turn {
                self.longest_turn = elapsed;
            }
        }
    }

    /// Total time spent working, including the interval currently open.
    ///
    /// Omitting the open interval would report a session that has been working
    /// for an hour as frozen at its last transition.
    pub fn working_time(&self, now: Instant) -> Duration {
        match self.working_since {
            Some(since) => self.working_total + now.saturating_duration_since(since),
            None => self.working_total,
        }
    }

    /// The longest completed turn.
    pub fn longest_turn(&self) -> Duration {
        self.longest_turn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(count: u64) -> Duration {
        Duration::from_secs(count * 60)
    }

    #[test]
    fn a_session_that_has_never_worked_reports_no_working_time() {
        let timing = SessionTiming::default();

        assert_eq!(timing.working_time(Instant::now()), Duration::ZERO);
    }

    #[test]
    fn working_time_includes_the_currently_open_interval() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.set_working(true, start);

        assert_eq!(timing.working_time(start + minutes(30)), minutes(30));
    }

    #[test]
    fn leaving_working_freezes_the_accumulated_total() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.set_working(true, start);
        timing.set_working(false, start + minutes(5));

        assert_eq!(timing.working_time(start + minutes(45)), minutes(5));
    }

    #[test]
    fn separate_working_intervals_accumulate() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.set_working(true, start);
        timing.set_working(false, start + minutes(1));
        timing.set_working(true, start + minutes(11));

        assert_eq!(timing.working_time(start + minutes(50)), minutes(40));
    }

    #[test]
    fn staying_working_does_not_restart_the_open_interval() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.set_working(true, start);
        timing.set_working(true, start + minutes(20));

        assert_eq!(timing.working_time(start + minutes(30)), minutes(30));
    }

    #[test]
    fn a_completed_turn_is_measured_as_elapsed_time_including_waits() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.start_turn(start);
        timing.set_working(true, start);
        timing.set_working(false, start + minutes(1));
        timing.set_working(true, start + minutes(11));
        timing.end_turn(start + minutes(50));

        assert_eq!(timing.longest_turn(), minutes(50));
    }

    #[test]
    fn a_shorter_later_turn_does_not_replace_the_longest_one() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.start_turn(start);
        timing.end_turn(start + minutes(50));
        timing.start_turn(start + minutes(60));
        timing.end_turn(start + minutes(62));

        assert_eq!(timing.longest_turn(), minutes(50));
    }

    #[test]
    fn a_longer_later_turn_replaces_the_longest_one() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.start_turn(start);
        timing.end_turn(start + minutes(2));
        timing.start_turn(start + minutes(10));
        timing.end_turn(start + minutes(60));

        assert_eq!(timing.longest_turn(), minutes(50));
    }

    #[test]
    fn an_open_turn_does_not_count_towards_the_longest_turn_until_it_completes() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.start_turn(start);

        assert_eq!(timing.longest_turn(), Duration::ZERO);
    }

    #[test]
    fn a_second_prompt_inside_an_open_turn_does_not_restart_the_turn_clock() {
        let start = Instant::now();
        let mut timing = SessionTiming::default();

        timing.start_turn(start);
        timing.start_turn(start + minutes(5));
        timing.end_turn(start + minutes(20));

        assert_eq!(timing.longest_turn(), minutes(20));
    }

    #[test]
    fn completing_a_turn_that_was_never_started_records_nothing() {
        let mut timing = SessionTiming::default();

        timing.end_turn(Instant::now());

        assert_eq!(timing.longest_turn(), Duration::ZERO);
    }
}
