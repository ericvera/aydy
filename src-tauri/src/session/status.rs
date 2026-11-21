//! The four session statuses and the facts they are derived from.

use std::time::Duration;

/// How long a session may stay Unknown before it is removed from the list.
///
/// This is half of one budget, not a setting of its own. A stopped session has
/// to leave the list within 2 minutes; the reconciliation scan runs every 25
/// seconds, and it is also what sweeps expired sessions out, so the window is
/// served in whole scans. Worst case a session waits one interval for the scan
/// that reports it gone and three more for the window: 25 s + 75 s = 100 s.
///
/// The scan interval lives in the discovery module. The two numbers are chosen
/// together, neither can move on its own, and a window that is not a whole
/// number of intervals costs a whole extra interval.
pub const UNKNOWN_RESOLUTION_WINDOW: Duration = Duration::from_secs(75);

/// The status of a session. Every listed session carries exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionStatus {
    /// Claude is waiting on the user.
    NeedsInput,
    /// A turn is running.
    Working,
    /// A turn has completed, or the session is sitting at the prompt.
    Done,
    /// The app cannot tell which of the other three applies.
    Unknown,
}

impl SessionStatus {
    /// The position this status takes when sessions are ordered within a group:
    /// Needs input, then Working, then Done, then Unknown.
    pub fn rank(self) -> u8 {
        match self {
            Self::NeedsInput => 0,
            Self::Working => 1,
            Self::Done => 2,
            Self::Unknown => 3,
        }
    }
}

/// The recorded facts a session's status is computed from.
///
/// Status is never stored. Handlers record facts and [`StatusFacts::status`]
/// derives the status from them, which is what keeps the precedence rule
/// (Needs input outranks Working) and the no-decay rule (a silent turn stays
/// Working) from contradicting each other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusFacts {
    /// Reconciliation positively reported this session gone. Absence from a
    /// reconciliation result is not this fact — it is no fact at all.
    pub reported_gone: bool,
    /// A wait is outstanding: Claude signalled it needs the user and has not
    /// resumed or ended its turn.
    pub waiting: bool,
    /// A turn is open: a prompt was submitted and has not completed.
    pub turn_open: bool,
    /// The app has positive evidence the session exists — a hook event, or a
    /// transcript showing it sitting at the prompt.
    pub presence_confirmed: bool,
}

impl StatusFacts {
    /// Derives the status these facts imply.
    pub fn status(self) -> SessionStatus {
        if self.reported_gone {
            SessionStatus::Unknown
        } else if self.waiting {
            SessionStatus::NeedsInput
        } else if self.turn_open {
            SessionStatus::Working
        } else if self.presence_confirmed {
            SessionStatus::Done
        } else {
            SessionStatus::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_with_no_recorded_facts_is_unknown() {
        let facts = StatusFacts::default();

        assert_eq!(facts.status(), SessionStatus::Unknown);
    }

    #[test]
    fn a_confirmed_session_with_no_open_turn_is_done() {
        let facts = StatusFacts {
            presence_confirmed: true,
            ..StatusFacts::default()
        };

        assert_eq!(facts.status(), SessionStatus::Done);
    }

    #[test]
    fn an_open_turn_is_working() {
        let facts = StatusFacts {
            presence_confirmed: true,
            turn_open: true,
            ..StatusFacts::default()
        };

        assert_eq!(facts.status(), SessionStatus::Working);
    }

    #[test]
    fn an_outstanding_wait_outranks_an_open_turn() {
        let facts = StatusFacts {
            presence_confirmed: true,
            turn_open: true,
            waiting: true,
            ..StatusFacts::default()
        };

        assert_eq!(facts.status(), SessionStatus::NeedsInput);
    }

    #[test]
    fn an_outstanding_wait_outside_a_turn_is_still_needs_input() {
        let facts = StatusFacts {
            presence_confirmed: true,
            waiting: true,
            ..StatusFacts::default()
        };

        assert_eq!(facts.status(), SessionStatus::NeedsInput);
    }

    #[test]
    fn a_session_reported_gone_is_unknown_whatever_else_is_recorded() {
        let facts = StatusFacts {
            reported_gone: true,
            presence_confirmed: true,
            turn_open: true,
            waiting: true,
        };

        assert_eq!(facts.status(), SessionStatus::Unknown);
    }

    #[test]
    fn statuses_rank_needs_input_working_done_unknown() {
        let mut statuses = [
            SessionStatus::Unknown,
            SessionStatus::Done,
            SessionStatus::Working,
            SessionStatus::NeedsInput,
        ];

        statuses.sort_by_key(|status| status.rank());

        assert_eq!(
            statuses,
            [
                SessionStatus::NeedsInput,
                SessionStatus::Working,
                SessionStatus::Done,
                SessionStatus::Unknown,
            ]
        );
    }

    #[test]
    fn the_unknown_resolution_window_is_seventy_five_seconds() {
        assert_eq!(UNKNOWN_RESOLUTION_WINDOW, Duration::from_secs(75));
    }
}
