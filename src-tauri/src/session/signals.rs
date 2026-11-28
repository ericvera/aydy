//! The stale and progress signals of a Working session.
//!
//! Stale is a signal beside the status, not a status: a silent turn stays
//! Working, and these signals only describe how that turn is going. The VS Code
//! extension derives the same signals by the same rule.

use std::time::{Duration, Instant};

/// How long a Working session may write nothing before it reads as stale.
pub const STALE_AFTER: Duration = Duration::from_secs(5 * 60);

/// An agent-reported step count, such as step 3 of 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub done: u32,
    pub total: u32,
}

impl Step {
    /// A step count, or `None` when it has no steps or more done than total.
    pub fn new(done: u32, total: u32) -> Option<Step> {
        (total >= 1 && done <= total).then_some(Step { done, total })
    }
}

/// The agent's latest report of how long its turn has left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressReport {
    pub reported_at: Instant,
    pub eta: Duration,
    pub step: Option<Step>,
}

/// The reported ETA as it stands now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eta {
    /// Time left on the reported ETA; negative once it is overdue.
    pub remaining_ms: i64,
    pub reported_ago: Duration,
}

/// The derived signals of a Working session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkSignals {
    pub stale: bool,
    /// Time since the session's last activity, or `None` while the transcript's
    /// written-at is unknown.
    pub quiet_for: Option<Duration>,
    pub turn_elapsed: Option<Duration>,
    pub step: Option<Step>,
    pub eta: Option<Eta>,
    /// Progress through the turn in thousandths.
    pub fraction_permille: Option<u16>,
}

/// What the signals are derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalInputs {
    pub turn_started_at: Option<Instant>,
    pub last_event_at: Instant,
    /// The newest write to the session's transcripts, subagents' included.
    pub transcript_written_at: Option<Instant>,
    pub progress: Option<ProgressReport>,
}

/// Derives the signals at `now`.
///
/// A report written before the turn started belongs to an earlier turn, so it
/// contributes nothing: not a step, not an ETA, not activity.
pub fn derive(inputs: SignalInputs, now: Instant) -> WorkSignals {
    let accepted = inputs.progress.filter(|report| {
        inputs
            .turn_started_at
            .is_none_or(|started| report.reported_at >= started)
    });

    let quiet_for = inputs.transcript_written_at.map(|written_at| {
        let activity = accepted.map_or(written_at, |report| report.reported_at.max(written_at));
        let activity = activity.max(inputs.last_event_at);
        now.saturating_duration_since(activity)
    });

    let turn_elapsed = inputs
        .turn_started_at
        .map(|started| now.saturating_duration_since(started));

    let eta = accepted.map(|report| {
        let reported_ago = now.saturating_duration_since(report.reported_at);
        Eta {
            remaining_ms: millis(report.eta).saturating_sub(millis(reported_ago)),
            reported_ago,
        }
    });

    let step = accepted.and_then(|report| report.step);

    let fraction_permille = match (eta, turn_elapsed, step) {
        (Some(eta), Some(elapsed), _) => Some(eta_fraction(elapsed, eta)),
        (_, _, Some(step)) => Some(step_fraction(step)),
        _ => None,
    };

    WorkSignals {
        stale: quiet_for.is_some_and(|quiet| quiet >= STALE_AFTER),
        quiet_for,
        turn_elapsed,
        step,
        eta,
        fraction_permille,
    }
}

fn eta_fraction(elapsed: Duration, eta: Eta) -> u16 {
    let elapsed = i128::from(millis(elapsed));
    let denominator = elapsed + i128::from(eta.remaining_ms.max(0));
    if denominator == 0 {
        return 1000;
    }
    u16::try_from((elapsed * 1000 / denominator).min(1000)).unwrap_or(1000)
}

fn step_fraction(step: Step) -> u16 {
    u16::try_from(u64::from(step.done) * 1000 / u64::from(step.total)).unwrap_or(1000)
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(count: u64) -> Duration {
        Duration::from_secs(count * 60)
    }

    fn inputs(start: Instant) -> SignalInputs {
        SignalInputs {
            turn_started_at: Some(start),
            last_event_at: start,
            transcript_written_at: Some(start),
            progress: None,
        }
    }

    fn report(reported_at: Instant, eta: Duration, step: Option<Step>) -> ProgressReport {
        ProgressReport {
            reported_at,
            eta,
            step,
        }
    }

    #[test]
    fn quiet_for_under_five_minutes_is_not_stale() {
        let start = Instant::now();

        let signals = derive(inputs(start), start + STALE_AFTER - Duration::from_secs(1));

        assert!(!signals.stale);
    }

    #[test]
    fn quiet_for_exactly_five_minutes_is_stale() {
        let start = Instant::now();

        let signals = derive(inputs(start), start + STALE_AFTER);

        assert!(signals.stale);
        assert_eq!(signals.quiet_for, Some(STALE_AFTER));
    }

    #[test]
    fn an_unknown_transcript_written_at_is_never_stale() {
        let start = Instant::now();
        let unknown = SignalInputs {
            transcript_written_at: None,
            ..inputs(start)
        };

        let signals = derive(unknown, start + minutes(600));

        assert_eq!(signals.quiet_for, None);
        assert!(!signals.stale);
    }

    #[test]
    fn a_hook_event_newer_than_the_transcript_resets_quiet() {
        let start = Instant::now();
        let later_event = SignalInputs {
            last_event_at: start + minutes(4),
            ..inputs(start)
        };

        let signals = derive(later_event, start + minutes(6));

        assert_eq!(signals.quiet_for, Some(minutes(2)));
        assert!(!signals.stale);
    }

    #[test]
    fn an_accepted_report_counts_as_activity() {
        let start = Instant::now();
        let reported = SignalInputs {
            progress: Some(report(start + minutes(4), minutes(10), None)),
            ..inputs(start)
        };

        let signals = derive(reported, start + minutes(6));

        assert_eq!(signals.quiet_for, Some(minutes(2)));
    }

    #[test]
    fn a_report_from_before_the_turn_is_ignored() {
        let start = Instant::now();
        let turn_start = start + minutes(10);
        let stale_report = SignalInputs {
            turn_started_at: Some(turn_start),
            last_event_at: start,
            transcript_written_at: Some(start),
            progress: Some(report(start + minutes(5), minutes(30), Step::new(2, 4))),
        };

        let signals = derive(stale_report, turn_start + minutes(1));

        assert_eq!(signals.step, None);
        assert_eq!(signals.eta, None);
        assert_eq!(signals.quiet_for, Some(minutes(11)));
        assert!(signals.stale);
    }

    #[test]
    fn the_accepted_reports_step_is_the_step() {
        let start = Instant::now();
        let reported = SignalInputs {
            progress: Some(report(start, minutes(10), Step::new(3, 7))),
            ..inputs(start)
        };

        let signals = derive(reported, start + minutes(1));

        assert_eq!(signals.step, Step::new(3, 7));
    }

    #[test]
    fn no_accepted_report_means_no_step() {
        let start = Instant::now();

        let signals = derive(inputs(start), start + minutes(1));

        assert_eq!(signals.step, None);
    }

    #[test]
    fn the_eta_counts_down_and_goes_negative_when_overdue() {
        let start = Instant::now();
        let reported = SignalInputs {
            progress: Some(report(start, minutes(10), None)),
            ..inputs(start)
        };

        let before = derive(reported, start + minutes(4));
        let after = derive(reported, start + minutes(12));

        assert_eq!(
            before.eta,
            Some(Eta {
                remaining_ms: 6 * 60 * 1000,
                reported_ago: minutes(4),
            })
        );
        assert_eq!(
            after.eta,
            Some(Eta {
                remaining_ms: -2 * 60 * 1000,
                reported_ago: minutes(12),
            })
        );
    }

    #[test]
    fn the_fraction_comes_from_the_eta_when_there_is_one() {
        let start = Instant::now();
        let reported = SignalInputs {
            progress: Some(report(start + minutes(2), minutes(8), Step::new(1, 10))),
            ..inputs(start)
        };

        let signals = derive(reported, start + minutes(4));

        assert_eq!(signals.fraction_permille, Some(400));
    }

    #[test]
    fn an_overdue_eta_is_a_full_fraction() {
        let start = Instant::now();
        let reported = SignalInputs {
            progress: Some(report(start, minutes(1), None)),
            ..inputs(start)
        };

        let signals = derive(reported, start + minutes(3));

        assert_eq!(signals.fraction_permille, Some(1000));
    }

    #[test]
    fn the_fraction_comes_from_the_step_without_a_turn_start() {
        let start = Instant::now();
        let reported = SignalInputs {
            turn_started_at: None,
            progress: Some(report(start, minutes(8), Step::new(1, 3))),
            ..inputs(start)
        };

        let signals = derive(reported, start + minutes(1));

        assert_eq!(signals.fraction_permille, Some(333));
    }

    #[test]
    fn there_is_no_fraction_without_an_eta_or_a_step() {
        let start = Instant::now();

        let signals = derive(inputs(start), start + minutes(1));

        assert_eq!(signals.fraction_permille, None);
        assert_eq!(signals.turn_elapsed, Some(minutes(1)));
    }

    #[test]
    fn a_step_needs_at_least_one_step_and_no_more_done_than_total() {
        assert_eq!(Step::new(0, 0), None);
        assert_eq!(Step::new(4, 3), None);
        assert_eq!(Step::new(3, 3), Some(Step { done: 3, total: 3 }));
    }
}
