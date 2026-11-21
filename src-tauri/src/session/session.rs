//! One tracked session: its identity, the facts its status is derived from,
//! and its timing.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::session::event::{HookEventKind, SessionId, WindowId};
use crate::session::status::{SessionStatus, StatusFacts, UNKNOWN_RESOLUTION_WINDOW};
use crate::session::timing::SessionTiming;

/// A single Claude Code session.
///
/// Status is not a field. Handlers record facts and [`Session::status`] derives
/// the status from them.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    /// The client the session belongs to, as reported by reconciliation.
    pub entrypoint: Option<String>,
    /// The resolved row label, supplied by transcript enrichment.
    pub label: Option<String>,
    pub repo: Option<String>,
    /// The linked worktree the session's working directory sits in, when it
    /// sits in one. The group header shows the repository, so this is detail
    /// rather than grouping: it is what tells two sessions on the same
    /// repository in different directories apart.
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub jumpable: bool,
    /// The VS Code window running this session, when the process tree places
    /// it in one. `None` means nothing proved which window it belongs to —
    /// never that a window was ruled out — so the reveal that would open a
    /// duplicate session is never sent.
    pub window: Option<WindowId>,
    /// When the session's last event, if it was a `Stop` or `Notification`,
    /// was recorded, in epoch ms. Any other event clears it.
    pub attention_at_ms: Option<u64>,
    /// When the session was last looked at, in epoch ms, from its seen file or
    /// a click on its row.
    pub seen_at_ms: Option<u64>,
    /// The message from the most recent `Notification`, describing what the
    /// session is waiting on. It is the only source for that detail.
    pub notification_message: Option<String>,
    hook_tracked: bool,
    last_event_at: Instant,
    facts: StatusFacts,
    timing: SessionTiming,
    unknown_since: Option<Instant>,
    scan_confirmed: bool,
}

impl Session {
    /// A session with no recorded facts, which is therefore Unknown.
    pub fn new(id: SessionId, now: Instant) -> Self {
        Self {
            id,
            cwd: None,
            transcript_path: None,
            entrypoint: None,
            label: None,
            repo: None,
            worktree: None,
            branch: None,
            jumpable: false,
            window: None,
            attention_at_ms: None,
            seen_at_ms: None,
            notification_message: None,
            hook_tracked: false,
            last_event_at: now,
            facts: StatusFacts::default(),
            timing: SessionTiming::default(),
            unknown_since: Some(now),
            scan_confirmed: false,
        }
    }

    /// Whether the session wants the user back and they have not looked since:
    /// its last event asked for attention after it was last seen. The VS Code
    /// extension applies the same rule, so the two counts agree.
    pub fn unread(&self) -> bool {
        matches!((self.attention_at_ms, self.seen_at_ms.unwrap_or(0)), (Some(a), s) if a > s)
    }

    /// The status these facts imply.
    pub fn status(&self) -> SessionStatus {
        self.facts.status()
    }

    /// When this session last produced an event, which is what orders it within
    /// its group. Reconciliation scans are not events.
    pub fn last_event_at(&self) -> Instant {
        self.last_event_at
    }

    /// Whether a hook event has ever been received for this session.
    ///
    /// While it is false the transcript is the only account of what the session
    /// is doing, so a derived status is authoritative.
    pub fn is_hook_tracked(&self) -> bool {
        self.hook_tracked
    }

    /// Total time spent working, including the interval currently open.
    pub fn working_time(&self, now: Instant) -> Duration {
        self.timing.working_time(now)
    }

    /// The longest completed turn, measured as elapsed time including waits.
    pub fn longest_turn(&self) -> Duration {
        self.timing.longest_turn()
    }

    /// Applies a hook event. `SessionEnd` is handled by the registry, which
    /// removes the session outright, so it records nothing here.
    pub fn apply_hook(&mut self, kind: &HookEventKind, now: Instant) {
        self.hook_tracked = true;
        self.facts.reported_gone = false;
        self.facts.presence_confirmed = true;

        match kind {
            HookEventKind::SessionStart | HookEventKind::SessionEnd => {}
            HookEventKind::UserPromptSubmit => {
                self.facts.waiting = false;
                self.facts.turn_open = true;
                self.timing.start_turn(now);
            }
            HookEventKind::Notification { message } => {
                self.facts.waiting = true;
                self.notification_message = message.clone();
            }
            HookEventKind::Stop => {
                self.facts.waiting = false;
                self.facts.turn_open = false;
                self.timing.end_turn(now);
            }
        }

        self.last_event_at = now;
        self.settle(now);
    }

    /// Records that the session has resumed work after a mid-turn wait.
    ///
    /// A session that was not waiting is left alone, and one that was waiting
    /// outside a turn returns to Done rather than to Working.
    pub fn resume(&mut self, now: Instant) {
        if !self.facts.waiting {
            return;
        }

        self.facts.waiting = false;
        self.last_event_at = now;
        self.settle(now);
    }

    /// Adopts a status derived elsewhere, by recording the facts it implies.
    ///
    /// A derived Working turn has no known start, so the longest-turn clock
    /// starts now and that turn is under-reported. An adopted Unknown records
    /// nothing: it is the absence of information, not information.
    ///
    /// An adopted wait drops any description the session was carrying. The
    /// hook is the only thing that can say what a session is waiting on, so a
    /// wait derived anywhere else has nothing to say about itself, and the
    /// description still on the session belongs to a prompt that was answered
    /// before this one began.
    pub fn adopt_status(&mut self, status: SessionStatus, now: Instant) {
        match status {
            SessionStatus::NeedsInput => {
                self.facts.presence_confirmed = true;
                self.facts.waiting = true;
                self.notification_message = None;
            }
            SessionStatus::Working => {
                self.facts.presence_confirmed = true;
                self.facts.waiting = false;
                self.facts.turn_open = true;
                self.timing.start_turn(now);
            }
            SessionStatus::Done => {
                self.facts.presence_confirmed = true;
                self.facts.waiting = false;
                self.facts.turn_open = false;
                self.timing.end_turn(now);
            }
            SessionStatus::Unknown => {}
        }

        self.settle(now);
    }

    /// Records that reconciliation can see this session.
    ///
    /// This is not an event, so it does not change the session's position in
    /// the list, and it does not restart the Unknown resolution window — a
    /// session the scan keeps seeing but cannot identify still has to resolve.
    /// It deliberately does not confirm presence either: a live process says
    /// the session exists, not that it has stopped working.
    pub fn mark_present(&mut self, now: Instant) {
        self.facts.reported_gone = false;
        self.scan_confirmed = true;
        self.settle(now);
    }

    /// Records that reconciliation positively reports this session gone.
    pub fn mark_gone(&mut self, now: Instant) {
        self.facts.reported_gone = true;
        self.scan_confirmed = false;
        self.settle(now);
    }

    /// Whether the last thing reconciliation said about this session is that it
    /// is running.
    ///
    /// This is existence evidence and nothing more, which is why it takes no
    /// part in [`Session::status`]. It matters only at the end of the Unknown
    /// resolution window, where the choice is between removing a session and
    /// settling on a status for it: a session whose process is demonstrably
    /// alive must not be removed.
    pub fn is_scan_confirmed(&self) -> bool {
        self.scan_confirmed
    }

    /// Whether this session has been Unknown for longer than the resolution
    /// window and must now leave the list or settle on a status.
    pub fn unknown_window_elapsed(&self, now: Instant) -> bool {
        self.unknown_since
            .is_some_and(|since| now.saturating_duration_since(since) >= UNKNOWN_RESOLUTION_WINDOW)
    }

    /// Brings the derived state in line with the facts just recorded.
    ///
    /// The working clock follows the derived status rather than being started
    /// and stopped by individual handlers, and the Unknown resolution window is
    /// opened and closed by the same derivation, so neither can disagree with
    /// the status the list shows.
    fn settle(&mut self, now: Instant) {
        let status = self.status();

        self.timing
            .set_working(status == SessionStatus::Working, now);

        if status == SessionStatus::Unknown {
            if self.unknown_since.is_none() {
                self.unknown_since = Some(now);
            }
        } else {
            self.unknown_since = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(count: u64) -> Duration {
        Duration::from_secs(count * 60)
    }

    fn notification(message: &str) -> HookEventKind {
        HookEventKind::Notification {
            message: Some(message.to_string()),
        }
    }

    #[test]
    fn a_new_session_is_unknown() {
        let session = Session::new("session-1".to_string(), Instant::now());

        assert_eq!(session.status(), SessionStatus::Unknown);
    }

    #[test]
    fn a_session_that_has_only_started_is_done() {
        let now = Instant::now();
        let mut session = Session::new("session-1".to_string(), now);

        session.apply_hook(&HookEventKind::SessionStart, now);

        assert_eq!(session.status(), SessionStatus::Done);
    }

    #[test]
    fn a_prompt_submission_starts_a_turn() {
        let now = Instant::now();
        let mut session = Session::new("session-1".to_string(), now);

        session.apply_hook(&HookEventKind::UserPromptSubmit, now);

        assert_eq!(session.status(), SessionStatus::Working);
    }

    #[test]
    fn a_notification_mid_turn_outranks_working() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::UserPromptSubmit, start);
        session.apply_hook(&notification("Needs permission"), start + minutes(1));

        assert_eq!(session.status(), SessionStatus::NeedsInput);
    }

    #[test]
    fn the_latest_notification_message_replaces_the_previous_one() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&notification("Needs permission for Bash"), start);
        session.apply_hook(
            &notification("Needs permission for Write"),
            start + minutes(1),
        );

        assert_eq!(
            session.notification_message,
            Some("Needs permission for Write".to_string())
        );
    }

    /// A wait adopted from somewhere other than the hook carries no
    /// description of itself. Without this a re-adopted wait would carry the
    /// tool and command of a permission prompt that was answered long before,
    /// which reads as a live request the user has already dealt with.
    #[test]
    fn an_adopted_wait_does_not_inherit_the_description_of_an_answered_one() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(
            &notification("Claude needs your permission to use Bash(git push:*)"),
            start,
        );
        session.resume(start + minutes(1));
        session.adopt_status(SessionStatus::NeedsInput, start + minutes(2));

        assert_eq!(session.status(), SessionStatus::NeedsInput);
        assert_eq!(session.notification_message, None);
    }

    #[test]
    fn resuming_returns_a_waiting_turn_to_working() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::UserPromptSubmit, start);
        session.apply_hook(&notification("Needs permission"), start + minutes(1));
        session.resume(start + minutes(11));

        assert_eq!(session.status(), SessionStatus::Working);
    }

    #[test]
    fn resuming_a_wait_outside_a_turn_returns_the_session_to_done() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::SessionStart, start);
        session.apply_hook(&notification("Waiting for input"), start + minutes(1));
        session.resume(start + minutes(2));

        assert_eq!(session.status(), SessionStatus::Done);
    }

    #[test]
    fn resuming_a_session_that_is_not_waiting_changes_nothing() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);
        session.apply_hook(&HookEventKind::UserPromptSubmit, start);

        session.resume(start + minutes(5));

        assert_eq!(session.status(), SessionStatus::Working);
        assert_eq!(session.last_event_at(), start);
    }

    #[test]
    fn a_completed_turn_returns_the_session_to_done() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::UserPromptSubmit, start);
        session.apply_hook(&HookEventKind::Stop, start + minutes(5));

        assert_eq!(session.status(), SessionStatus::Done);
    }

    #[test]
    fn completing_a_turn_clears_an_outstanding_wait() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::UserPromptSubmit, start);
        session.apply_hook(&notification("Needs permission"), start + minutes(1));
        session.apply_hook(&HookEventKind::Stop, start + minutes(2));

        assert_eq!(session.status(), SessionStatus::Done);
    }

    #[test]
    fn a_new_prompt_clears_an_outstanding_wait() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&notification("Waiting for input"), start);
        session.apply_hook(&HookEventKind::UserPromptSubmit, start + minutes(1));

        assert_eq!(session.status(), SessionStatus::Working);
    }

    #[test]
    fn a_wait_is_excluded_from_working_time_but_included_in_the_turn() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::UserPromptSubmit, start);
        session.apply_hook(&notification("Needs permission"), start + minutes(1));
        session.resume(start + minutes(11));
        session.apply_hook(&HookEventKind::Stop, start + minutes(50));

        assert_eq!(session.working_time(start + minutes(50)), minutes(40));
        assert_eq!(session.longest_turn(), minutes(50));
    }

    #[test]
    fn a_silent_turn_stays_working_and_keeps_accruing_time() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::UserPromptSubmit, start);

        assert_eq!(session.status(), SessionStatus::Working);
        assert_eq!(session.working_time(start + minutes(30)), minutes(30));
    }

    #[test]
    fn a_session_sitting_at_done_accrues_no_working_time() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::SessionStart, start);

        assert_eq!(session.working_time(start + minutes(60)), Duration::ZERO);
    }

    #[test]
    fn a_session_that_has_only_started_never_opens_the_unknown_window() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::SessionStart, start);

        assert!(!session.unknown_window_elapsed(start + minutes(2)));
    }

    #[test]
    fn an_unresolved_session_leaves_after_the_unknown_window() {
        let start = Instant::now();
        let session = Session::new("session-1".to_string(), start);

        assert!(!session
            .unknown_window_elapsed(start + UNKNOWN_RESOLUTION_WINDOW - Duration::from_secs(1)));
        assert!(session.unknown_window_elapsed(start + UNKNOWN_RESOLUTION_WINDOW));
    }

    #[test]
    fn being_seen_by_reconciliation_does_not_restart_the_unknown_window() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.mark_present(start + Duration::from_secs(20));
        session.mark_present(start + Duration::from_secs(40));

        assert!(session.unknown_window_elapsed(start + UNKNOWN_RESOLUTION_WINDOW));
    }

    #[test]
    fn being_seen_by_reconciliation_records_that_the_session_exists() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        assert!(!session.is_scan_confirmed());

        session.mark_present(start);

        assert!(session.is_scan_confirmed());
        // Existence is not activity: the session is still Unknown, and only the
        // end of the resolution window makes anything of this.
        assert_eq!(session.status(), SessionStatus::Unknown);
    }

    #[test]
    fn being_reported_gone_withdraws_the_scans_confirmation() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);
        session.mark_present(start);

        session.mark_gone(start + minutes(1));

        assert!(!session.is_scan_confirmed());
    }

    #[test]
    fn a_session_reported_gone_becomes_unknown() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);
        session.apply_hook(&HookEventKind::SessionStart, start);

        session.mark_gone(start + minutes(1));

        assert_eq!(session.status(), SessionStatus::Unknown);
    }

    #[test]
    fn a_hook_event_resolves_a_session_previously_reported_gone() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);
        session.mark_gone(start);

        session.apply_hook(&HookEventKind::Stop, start + minutes(1));

        assert_eq!(session.status(), SessionStatus::Done);
        assert!(!session.unknown_window_elapsed(start + minutes(10)));
    }

    #[test]
    fn adopting_a_derived_done_status_makes_the_session_done() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.adopt_status(SessionStatus::Done, start);

        assert_eq!(session.status(), SessionStatus::Done);
    }

    #[test]
    fn adopting_a_derived_working_status_starts_the_working_clock() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.adopt_status(SessionStatus::Working, start);

        assert_eq!(session.status(), SessionStatus::Working);
        assert_eq!(session.working_time(start + minutes(10)), minutes(10));
    }

    #[test]
    fn adopting_an_unknown_status_records_nothing() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.adopt_status(SessionStatus::Unknown, start);

        assert_eq!(session.status(), SessionStatus::Unknown);
        assert!(session.unknown_window_elapsed(start + UNKNOWN_RESOLUTION_WINDOW));
    }

    #[test]
    fn adopting_a_status_does_not_make_a_session_hook_tracked() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.adopt_status(SessionStatus::Done, start);

        assert!(!session.is_hook_tracked());
    }

    #[test]
    fn a_hook_event_makes_a_session_hook_tracked() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.apply_hook(&HookEventKind::SessionStart, start);

        assert!(session.is_hook_tracked());
    }

    #[test]
    fn adopting_done_over_an_adopted_working_turn_records_the_turn() {
        let start = Instant::now();
        let mut session = Session::new("session-1".to_string(), start);

        session.adopt_status(SessionStatus::Working, start);
        session.adopt_status(SessionStatus::Done, start + minutes(3));

        assert_eq!(session.status(), SessionStatus::Done);
        assert_eq!(session.longest_turn(), minutes(3));
        assert_eq!(session.working_time(start + minutes(10)), minutes(3));
    }
}
