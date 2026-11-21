//! Telling the user that a session wants them back.
//!
//! Everything here is driven by observations of the session registry rather
//! than by hook events: an alert belongs to the moment a session *enters* a
//! status, and comparing one observation to the previous one is what makes that
//! a transition rather than a state that fires again on every tick.
//!
//! The sound is what carries an alert; [`transition_alert`] is what decides
//! that one is due.

pub mod sound;

use crate::session::status::SessionStatus;

/// A session entering a status the user has to be told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alert {
    /// The session finished.
    Finished,
    /// The session began needing input.
    NeedsInput,
}

/// The alert a session earns by moving from `previous` to `status`.
///
/// A session observed for the first time earns nothing. The app starts with
/// sessions that finished hours ago, and reading a list at launch is not ten
/// things happening at once. Staying in a status earns nothing either: the
/// alert belongs to the moment the session entered it, and a status that is
/// merely observed again would fire on every tick.
pub fn transition_alert(previous: Option<SessionStatus>, status: SessionStatus) -> Option<Alert> {
    if previous? == status {
        return None;
    }

    match status {
        SessionStatus::Done => Some(Alert::Finished),
        SessionStatus::NeedsInput => Some(Alert::NeedsInput),
        SessionStatus::Working | SessionStatus::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_observed_for_the_first_time_earns_nothing() {
        assert_eq!(transition_alert(None, SessionStatus::Done), None);
        assert_eq!(transition_alert(None, SessionStatus::NeedsInput), None);
    }

    #[test]
    fn a_status_that_has_not_changed_earns_nothing() {
        assert_eq!(
            transition_alert(Some(SessionStatus::Done), SessionStatus::Done),
            None
        );
        assert_eq!(
            transition_alert(Some(SessionStatus::NeedsInput), SessionStatus::NeedsInput),
            None
        );
    }

    #[test]
    fn entering_done_or_needs_input_is_what_earns_an_alert() {
        assert_eq!(
            transition_alert(Some(SessionStatus::Working), SessionStatus::Done),
            Some(Alert::Finished)
        );
        assert_eq!(
            transition_alert(Some(SessionStatus::Working), SessionStatus::NeedsInput),
            Some(Alert::NeedsInput)
        );
    }

    #[test]
    fn working_and_unknown_earn_nothing() {
        assert_eq!(
            transition_alert(Some(SessionStatus::Done), SessionStatus::Working),
            None
        );
        assert_eq!(
            transition_alert(Some(SessionStatus::Working), SessionStatus::Unknown),
            None
        );
    }
}
