//! The collection of tracked sessions: event application, unread state,
//! grouping and ordering.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clock::Clock;
use crate::discovery::status_files::StatusRecord;
use crate::session::event::{
    HookEvent, HookEventKind, ReconciliationResult, SessionEnrichment, SessionId,
};
use crate::session::session::Session;
use crate::session::status::SessionStatus;

/// The label a row falls back to when nothing else is known about a session.
/// A row is never blank.
pub const DEFAULT_SESSION_LABEL: &str = "Untitled session";

/// Which kind of group a session belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionGroupKind {
    /// A jumpable session whose repository is known.
    Project,
    /// A jumpable session whose project is not yet identified.
    Unidentified,
    /// A session belonging to another client, or one whose working directory is
    /// not on this machine.
    NotJumpable,
}

/// One session, as the interface sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub label: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub status: SessionStatus,
    pub unread: bool,
    pub jumpable: bool,
    pub is_current: bool,
    pub working_time: Duration,
    pub longest_turn: Duration,
    /// What the session is waiting on, from its most recent notification.
    pub notification_message: Option<String>,
}

/// One group of sessions, already ordered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionGroup {
    pub kind: SessionGroupKind,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub sessions: Vec<SessionSnapshot>,
}

/// The whole list, grouped and ordered.
///
/// Grouping and ordering are done here rather than in the interface, so the
/// popover cannot disagree with the menu bar icon.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistrySnapshot {
    pub groups: Vec<SessionGroup>,
    pub session_count: usize,
    pub unread_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Project {
        repo: String,
        branch: Option<String>,
    },
    Unidentified,
    NotJumpable,
}

impl GroupKey {
    fn for_session(session: &Session) -> Self {
        if !session.jumpable {
            return Self::NotJumpable;
        }

        match &session.repo {
            Some(repo) => Self::Project {
                repo: repo.clone(),
                branch: session.branch.clone(),
            },
            None => Self::Unidentified,
        }
    }

    /// Identified groups come first, then the not-yet-identified group, then
    /// the not-jumpable group.
    fn rank(&self) -> u8 {
        match self {
            Self::Project { .. } => 0,
            Self::Unidentified => 1,
            Self::NotJumpable => 2,
        }
    }

    fn kind(&self) -> SessionGroupKind {
        match self {
            Self::Project { .. } => SessionGroupKind::Project,
            Self::Unidentified => SessionGroupKind::Unidentified,
            Self::NotJumpable => SessionGroupKind::NotJumpable,
        }
    }

    fn repo(&self) -> Option<String> {
        match self {
            Self::Project { repo, .. } => Some(repo.clone()),
            _ => None,
        }
    }

    fn branch(&self) -> Option<String> {
        match self {
            Self::Project { branch, .. } => branch.clone(),
            _ => None,
        }
    }
}

/// Every session the app is tracking.
pub struct SessionRegistry {
    clock: Arc<dyn Clock>,
    sessions: HashMap<SessionId, Session>,
}

impl SessionRegistry {
    /// An empty registry reading time from `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            sessions: HashMap::new(),
        }
    }

    /// A tracked session by id.
    pub fn session(&self, session_id: &str) -> Option<&Session> {
        self.sessions.get(session_id)
    }

    /// Every tracked session, in no particular order.
    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    /// Applies a hook event, creating the session if it has not been seen
    /// before. It does not touch unread, which only status records raise.
    pub fn apply_hook_event(&mut self, event: HookEvent) {
        let now = self.clock.now();
        self.apply_event(event, now);
    }

    /// Applies a status record as of the time it was recorded rather than the
    /// time it was read, so a `Stop` written yesterday sorts as yesterday's and
    /// a turn begun ten minutes ago has ten minutes of working time.
    ///
    /// Wall-clock time only reaches the monotonic clock through the record's
    /// age. A record dated in the future has no age and lands at now.
    ///
    /// Only a `Stop` or `Notification` asks for attention, and any other event
    /// withdraws it, so a session is unread exactly while its last event is one
    /// of those two and is newer than its seen time.
    pub fn apply_status(&mut self, record: StatusRecord, now_ms: u64) {
        let now = self.clock.now();
        let age = Duration::from_millis(now_ms.saturating_sub(record.written_at_ms));
        let at = now.checked_sub(age).unwrap_or(now);
        let session_id = record.event.session_id.clone();
        let attention = matches!(
            record.event.kind,
            HookEventKind::Stop | HookEventKind::Notification { .. }
        )
        .then_some(record.written_at_ms);

        self.apply_event(record.event, at);

        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.attention_at_ms = attention;
        }
    }

    /// Records that a session was seen at `seen_at_ms`, keeping the latest
    /// seen time. Unknown session ids are ignored.
    pub fn apply_seen(&mut self, session_id: &str, seen_at_ms: u64) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.seen_at_ms = Some(session.seen_at_ms.unwrap_or(0).max(seen_at_ms));
        }
    }

    fn apply_event(&mut self, event: HookEvent, at: Instant) {
        let HookEvent {
            session_id,
            cwd,
            transcript_path,
            kind,
        } = event;

        if matches!(kind, HookEventKind::SessionEnd) {
            self.sessions.remove(&session_id);
            return;
        }

        let session = self.entry(&session_id, at);

        if cwd.is_some() {
            session.cwd = cwd;
        }
        if transcript_path.is_some() {
            session.transcript_path = transcript_path;
        }

        session.apply_hook(&kind, at);
    }

    /// Applies one reconciliation scan.
    ///
    /// Sessions the scan cannot see are left exactly as they are: absence from
    /// the result is not evidence of death, and treating it as such would drop
    /// every Claude desktop and remote session minutes after its last event.
    ///
    /// A discovered status is adopted while the transcript is the only account
    /// of the session — that is, until a hook event arrives — and afterwards
    /// only to resolve an Unknown, so a scan cannot overwrite a wait that the
    /// transcript has no way of knowing about.
    pub fn apply_reconciliation(&mut self, result: ReconciliationResult) {
        let now = self.clock.now();

        for discovered in result.present {
            let status = discovered.status;
            let session = self.entry(&discovered.session_id, now);

            if discovered.cwd.is_some() {
                session.cwd = discovered.cwd;
            }
            if discovered.entrypoint.is_some() {
                session.entrypoint = discovered.entrypoint;
            }
            session.jumpable = discovered.jumpable;
            session.mark_present(now);

            if let Some(status) = status {
                if !session.is_hook_tracked() || session.status() == SessionStatus::Unknown {
                    session.adopt_status(status, now);
                }
            }
        }

        for session_id in result.gone {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.mark_gone(now);
            }
        }
    }

    /// Records that a session has resumed work after a mid-turn wait.
    pub fn apply_resume(&mut self, session_id: &str) {
        let now = self.clock.now();

        if let Some(session) = self.sessions.get_mut(session_id) {
            session.resume(now);
        }
    }

    /// Applies fields derived elsewhere. Unknown session ids are ignored.
    pub fn apply_enrichment(&mut self, session_id: &str, enrichment: SessionEnrichment) {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return;
        };

        if enrichment.label.is_some() {
            session.label = enrichment.label;
        }
        if enrichment.repo.is_some() {
            session.repo = enrichment.repo;
        }
        if enrichment.worktree.is_some() {
            session.worktree = enrichment.worktree;
        }
        if enrichment.branch.is_some() {
            session.branch = enrichment.branch;
        }
        if let Some(jumpable) = enrichment.jumpable {
            session.jumpable = jumpable;
        }
    }

    /// Marks a session read as of `now_ms`, which is what a click in the
    /// popover does.
    pub fn mark_read(&mut self, session_id: &str, now_ms: u64) {
        self.apply_seen(session_id, now_ms);
    }

    /// Settles every session that has been Unknown for longer than the
    /// resolution window: Unknown is transient, so each one either resolves to
    /// a status or leaves the list.
    ///
    /// A session reconciliation can still see is **resolved to Done**, not
    /// removed. Its process is alive and its file is on disk, so removing it
    /// would delete a session that demonstrably exists — and the next scan
    /// would add it straight back, which is a row that blinks out and returns
    /// every hundred seconds for as long as the session lives. Done is the
    /// safest of the three real statuses to settle on: it is what a session
    /// with no other evidence behind it already reports, and it accrues no
    /// working time, so a session wrongly settled here inflates nothing. The
    /// full window is spent waiting for the transcript to speak first, and a
    /// transcript that becomes readable later still overrides this.
    ///
    /// A session the scan cannot see is removed, exactly as before: a session
    /// reported gone, or one that was never in a scan at all, has no evidence
    /// of existence to weigh against the window running out.
    ///
    /// The reconciliation scan calls this on every pass and taking a snapshot
    /// does it too, so a session settles on time whether or not the interface
    /// ever asks for one.
    pub fn tick(&mut self) {
        let now = self.clock.now();
        let expired: Vec<SessionId> = self
            .sessions
            .values()
            .filter(|session| session.unknown_window_elapsed(now))
            .map(|session| session.id.clone())
            .collect();

        for session_id in expired {
            let Some(session) = self.sessions.get_mut(&session_id) else {
                continue;
            };

            if session.is_scan_confirmed() {
                session.adopt_status(SessionStatus::Done, now);
                continue;
            }

            self.sessions.remove(&session_id);
        }
    }

    /// The grouped, ordered, fully-derived view of every tracked session.
    pub fn snapshot(&mut self) -> RegistrySnapshot {
        self.tick();

        let now = self.clock.now();

        let mut ordered: Vec<&Session> = self.sessions.values().collect();
        ordered.sort_by(|left, right| {
            left.status()
                .rank()
                .cmp(&right.status().rank())
                .then_with(|| right.last_event_at().cmp(&left.last_event_at()))
                .then_with(|| left.id.cmp(&right.id))
        });

        let session_count = ordered.len();
        let unread_count = ordered.iter().filter(|session| session.unread()).count();

        let mut grouped: Vec<(GroupKey, Instant, Vec<SessionSnapshot>)> = Vec::new();

        for session in ordered {
            let key = GroupKey::for_session(session);
            let last_event_at = session.last_event_at();
            let snapshot = SessionSnapshot {
                id: session.id.clone(),
                label: session
                    .label
                    .clone()
                    .unwrap_or_else(|| DEFAULT_SESSION_LABEL.to_string()),
                repo: session.repo.clone(),
                branch: session.branch.clone(),
                status: session.status(),
                unread: session.unread(),
                jumpable: session.jumpable,
                // The field is removed in 03_07.
                is_current: false,
                working_time: session.working_time(now),
                longest_turn: session.longest_turn(),
                notification_message: session.notification_message.clone(),
            };

            match grouped.iter_mut().find(|(existing, _, _)| existing == &key) {
                Some((_, most_recent, members)) => {
                    if last_event_at > *most_recent {
                        *most_recent = last_event_at;
                    }
                    members.push(snapshot);
                }
                None => grouped.push((key, last_event_at, vec![snapshot])),
            }
        }

        grouped.sort_by(|left, right| {
            left.0
                .rank()
                .cmp(&right.0.rank())
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.0.cmp(&right.0))
        });

        RegistrySnapshot {
            groups: grouped
                .into_iter()
                .map(|(key, _, sessions)| SessionGroup {
                    kind: key.kind(),
                    repo: key.repo(),
                    branch: key.branch(),
                    sessions,
                })
                .collect(),
            session_count,
            unread_count,
        }
    }

    fn entry(&mut self, session_id: &str, now: Instant) -> &mut Session {
        self.sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session::new(session_id.to_string(), now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;
    use crate::session::event::{DiscoveredSession, HookEvent};
    use crate::session::status::UNKNOWN_RESOLUTION_WINDOW;

    fn minutes(count: u64) -> Duration {
        Duration::from_secs(count * 60)
    }

    fn registry() -> (Arc<TestClock>, SessionRegistry) {
        let clock = Arc::new(TestClock::new());
        let registry = SessionRegistry::new(clock.clone());

        (clock, registry)
    }

    fn notification(message: &str) -> HookEventKind {
        HookEventKind::Notification {
            message: Some(message.to_string()),
        }
    }

    fn hook(session_id: &str, kind: HookEventKind) -> HookEvent {
        HookEvent::new(session_id, kind)
    }

    fn find<'a>(snapshot: &'a RegistrySnapshot, session_id: &str) -> &'a SessionSnapshot {
        snapshot
            .groups
            .iter()
            .flat_map(|group| &group.sessions)
            .find(|session| session.id == session_id)
            .expect("the session is missing from the snapshot")
    }

    fn contains(snapshot: &RegistrySnapshot, session_id: &str) -> bool {
        snapshot
            .groups
            .iter()
            .flat_map(|group| &group.sessions)
            .any(|session| session.id == session_id)
    }

    fn statuses(snapshot: &RegistrySnapshot) -> Vec<SessionStatus> {
        snapshot
            .groups
            .iter()
            .flat_map(|group| &group.sessions)
            .map(|session| session.status)
            .collect()
    }

    fn ids(snapshot: &RegistrySnapshot) -> Vec<&str> {
        snapshot
            .groups
            .iter()
            .flat_map(|group| &group.sessions)
            .map(|session| session.id.as_str())
            .collect()
    }

    fn enrich(registry: &mut SessionRegistry, session_id: &str, repo: &str, branch: &str) {
        registry.apply_enrichment(
            session_id,
            SessionEnrichment {
                repo: Some(repo.to_string()),
                branch: Some(branch.to_string()),
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );
    }

    #[test]
    fn a_session_that_has_only_started_is_done() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-1", HookEventKind::SessionStart));

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Done
        );
    }

    #[test]
    fn a_session_that_has_only_started_is_still_listed_after_two_minutes() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::SessionStart));

        clock.advance(minutes(2));

        assert!(contains(&registry.snapshot(), "session-1"));
    }

    #[test]
    fn a_prompt_submission_makes_a_session_working() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Working
        );
    }

    #[test]
    fn a_completed_turn_makes_a_session_done() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Done
        );
    }

    #[test]
    fn a_session_that_ends_leaves_the_list() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        registry.apply_hook_event(hook("session-1", HookEventKind::SessionEnd));

        assert!(!contains(&registry.snapshot(), "session-1"));
    }

    #[test]
    fn a_session_that_ends_is_never_shown_as_unknown_first() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        registry.apply_hook_event(hook("session-1", HookEventKind::SessionEnd));

        assert!(registry.snapshot().groups.is_empty());
    }

    #[test]
    fn an_event_for_a_session_that_has_not_been_seen_before_starts_tracking_it() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-unseen", HookEventKind::Stop));

        assert!(contains(&registry.snapshot(), "session-unseen"));
    }

    #[test]
    fn a_notification_mid_turn_makes_a_session_needs_input_rather_than_working() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        clock.advance(minutes(1));
        registry.apply_hook_event(hook("session-1", notification("Needs permission")));

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::NeedsInput
        );
    }

    #[test]
    fn a_notification_message_is_stored_on_the_session_and_readable() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(hook(
            "session-1",
            notification("Needs permission for `Bash · git push`"),
        ));

        assert_eq!(
            find(&registry.snapshot(), "session-1").notification_message,
            Some("Needs permission for `Bash · git push`".to_string())
        );
    }

    #[test]
    fn a_second_notification_replaces_the_stored_message() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", notification("Needs permission for Bash")));

        clock.advance(minutes(1));
        registry.apply_hook_event(hook(
            "session-1",
            notification("Needs permission for Write"),
        ));

        assert_eq!(
            find(&registry.snapshot(), "session-1").notification_message,
            Some("Needs permission for Write".to_string())
        );
    }

    #[test]
    fn a_resume_signal_returns_a_waiting_session_to_working_inside_the_five_second_bound() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));
        registry.apply_hook_event(hook("session-1", notification("Needs permission")));

        clock.advance(Duration::from_secs(3));
        registry.apply_resume("session-1");

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Working
        );
    }

    #[test]
    fn a_wait_is_excluded_from_working_time_and_included_in_the_longest_turn() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));
        clock.advance(minutes(1));
        registry.apply_hook_event(hook("session-1", notification("Needs permission")));
        clock.advance(minutes(10));
        registry.apply_resume("session-1");
        clock.advance(minutes(39));
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        let snapshot = registry.snapshot();
        let session = find(&snapshot, "session-1");

        assert_eq!(session.working_time, minutes(40));
        assert_eq!(session.longest_turn, minutes(50));
    }

    #[test]
    fn a_silent_thirty_minute_turn_stays_working_and_accrues_thirty_minutes() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        clock.advance(minutes(30));

        let snapshot = registry.snapshot();
        let session = find(&snapshot, "session-1");

        assert_eq!(session.status, SessionStatus::Working);
        assert_eq!(session.working_time, minutes(30));
    }

    #[test]
    fn a_turn_running_for_two_hours_never_decays_to_unknown() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        for _ in 0..240 {
            clock.advance(Duration::from_secs(30));
            registry.tick();
        }

        let snapshot = registry.snapshot();
        let session = find(&snapshot, "session-1");

        assert_eq!(session.status, SessionStatus::Working);
        assert_eq!(session.working_time, minutes(120));
    }

    #[test]
    fn a_session_the_scan_keeps_seeing_but_cannot_identify_stays_unknown_for_the_whole_window() {
        let (clock, mut registry) = registry();
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").jumpable()],
            gone: Vec::new(),
        });

        clock.advance(UNKNOWN_RESOLUTION_WINDOW - Duration::from_secs(1));
        registry.tick();

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Unknown
        );
    }

    #[test]
    fn a_session_the_scan_keeps_seeing_but_cannot_identify_settles_at_done_rather_than_flapping() {
        // Its file is on disk and its process is alive; only its transcript is
        // silent. Removing it would delete a session that demonstrably exists,
        // and the next scan would add it straight back — a row that blinks out
        // and returns for as long as the session runs.
        let (clock, mut registry) = registry();

        for _ in 0..8 {
            registry.apply_reconciliation(ReconciliationResult {
                present: vec![DiscoveredSession::new("session-1").jumpable()],
                gone: Vec::new(),
            });
            registry.tick();
            clock.advance(Duration::from_secs(25));
        }

        let snapshot = registry.snapshot();

        assert!(contains(&snapshot, "session-1"));
        assert_eq!(find(&snapshot, "session-1").status, SessionStatus::Done);
    }

    #[test]
    fn a_session_that_settled_at_done_still_takes_the_status_its_transcript_reports_later() {
        let (clock, mut registry) = registry();
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").jumpable()],
            gone: Vec::new(),
        });
        clock.advance(UNKNOWN_RESOLUTION_WINDOW);
        registry.tick();

        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1")
                .jumpable()
                .with_status(SessionStatus::Working)],
            gone: Vec::new(),
        });

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Working
        );
    }

    #[test]
    fn a_session_the_scan_reports_gone_leaves_the_list_when_its_window_runs_out() {
        let (clock, mut registry) = registry();
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1")
                .jumpable()
                .with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });

        registry.apply_reconciliation(ReconciliationResult {
            present: Vec::new(),
            gone: vec!["session-1".to_string()],
        });
        clock.advance(UNKNOWN_RESOLUTION_WINDOW);
        registry.tick();

        assert!(registry.session("session-1").is_none());
    }

    #[test]
    fn a_session_reconciliation_discovers_is_not_marked_unread() {
        let (_clock, mut registry) = registry();

        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });

        assert!(!find(&registry.snapshot(), "session-1").unread);
    }

    #[test]
    fn a_session_reconciliation_discovers_keeps_the_status_it_was_supplied() {
        let (_clock, mut registry) = registry();

        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Done
        );
    }

    #[test]
    fn a_discovered_session_supplied_with_done_survives_past_two_minutes() {
        let (clock, mut registry) = registry();
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });

        clock.advance(minutes(2));

        assert!(contains(&registry.snapshot(), "session-1"));
    }

    #[test]
    fn a_discovered_session_with_no_derivable_status_is_unknown() {
        let (_clock, mut registry) = registry();

        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1")],
            gone: Vec::new(),
        });

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Unknown
        );
    }

    #[test]
    fn a_rediscovered_done_session_is_not_re_derived_once_hooks_are_tracking_it() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::UserPromptSubmit));

        clock.advance(minutes(1));
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Working
        );
    }

    #[test]
    fn a_supplied_status_resolves_a_session_that_was_unknown() {
        let (clock, mut registry) = registry();
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1")],
            gone: Vec::new(),
        });

        clock.advance(Duration::from_secs(30));
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });
        clock.advance(minutes(5));

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Done
        );
    }

    #[test]
    fn a_session_absent_from_every_reconciliation_result_keeps_its_status_and_stays() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-remote", HookEventKind::Stop));

        for _ in 0..10 {
            clock.advance(Duration::from_secs(30));
            registry.apply_reconciliation(ReconciliationResult {
                present: vec![DiscoveredSession::new("session-local").jumpable()],
                gone: Vec::new(),
            });
        }

        let snapshot = registry.snapshot();

        assert!(contains(&snapshot, "session-remote"));
        assert_eq!(
            find(&snapshot, "session-remote").status,
            SessionStatus::Done
        );
    }

    #[test]
    fn a_working_session_absent_from_every_reconciliation_result_stays_working() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-remote", HookEventKind::UserPromptSubmit));

        clock.advance(minutes(10));
        registry.apply_reconciliation(ReconciliationResult::default());

        assert_eq!(
            find(&registry.snapshot(), "session-remote").status,
            SessionStatus::Working
        );
    }

    #[test]
    fn a_session_positively_reported_gone_becomes_unknown() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        clock.advance(Duration::from_secs(30));
        registry.apply_reconciliation(ReconciliationResult {
            present: Vec::new(),
            gone: vec!["session-1".to_string()],
        });

        assert_eq!(
            find(&registry.snapshot(), "session-1").status,
            SessionStatus::Unknown
        );
    }

    #[test]
    fn a_session_positively_reported_gone_is_removed() {
        let (clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        registry.apply_reconciliation(ReconciliationResult {
            present: Vec::new(),
            gone: vec!["session-1".to_string()],
        });

        clock.advance(UNKNOWN_RESOLUTION_WINDOW);

        assert!(!contains(&registry.snapshot(), "session-1"));
    }

    #[test]
    fn an_unknown_session_that_does_not_resolve_within_the_window_is_removed() {
        let (clock, mut registry) = registry();
        // A session no scan can see: nothing weighs against the window running
        // out, so the window is what decides.
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        registry.apply_reconciliation(ReconciliationResult {
            present: Vec::new(),
            gone: vec!["session-1".to_string()],
        });

        clock.advance(UNKNOWN_RESOLUTION_WINDOW - Duration::from_secs(1));
        assert!(contains(&registry.snapshot(), "session-1"));

        clock.advance(Duration::from_secs(1));
        assert!(!contains(&registry.snapshot(), "session-1"));
    }

    #[test]
    fn an_unknown_session_resolved_by_a_hook_event_survives_the_resolution_window() {
        let (clock, mut registry) = registry();
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1")],
            gone: Vec::new(),
        });

        clock.advance(Duration::from_secs(30));
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        clock.advance(minutes(10));

        assert!(contains(&registry.snapshot(), "session-1"));
    }

    #[test]
    fn ticking_removes_an_expired_unknown_session_without_taking_a_snapshot() {
        let (clock, mut registry) = registry();
        // A session no scan has ever been able to see, so nothing weighs
        // against the window running out.
        registry.apply_reconciliation(ReconciliationResult {
            present: Vec::new(),
            gone: vec!["session-1".to_string()],
        });
        registry.apply_hook_event(hook("session-1", HookEventKind::SessionStart));
        registry.apply_reconciliation(ReconciliationResult {
            present: Vec::new(),
            gone: vec!["session-1".to_string()],
        });

        clock.advance(UNKNOWN_RESOLUTION_WINDOW);
        registry.tick();

        assert!(registry.session("session-1").is_none());
    }

    #[test]
    fn marking_an_unknown_session_id_read_does_nothing() {
        let (_clock, mut registry) = registry();

        registry.mark_read("session-never-seen", NOW_MS);

        assert_eq!(registry.snapshot().session_count, 0);
    }

    /// A wall-clock instant the status and seen tests are dated against.
    const NOW_MS: u64 = 1_790_000_000_000;

    const HOUR_MS: u64 = 60 * 60 * 1000;

    /// A registry whose monotonic clock is far enough from its origin that a
    /// record dated hours back can be placed exactly.
    fn dated_registry() -> (Arc<TestClock>, SessionRegistry) {
        let (clock, registry) = registry();
        clock.advance(Duration::from_secs(24 * 60 * 60));

        (clock, registry)
    }

    fn status(session_id: &str, kind: HookEventKind, written_at_ms: u64) -> StatusRecord {
        StatusRecord {
            event: hook(session_id, kind),
            written_at_ms,
        }
    }

    fn is_unread(registry: &mut SessionRegistry, session_id: &str) -> bool {
        find(&registry.snapshot(), session_id).unread
    }

    #[test]
    fn a_stop_is_unread_until_a_newer_seen_time_arrives() {
        let (_clock, mut registry) = dated_registry();
        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 1_000),
            NOW_MS,
        );
        assert!(is_unread(&mut registry, "session-1"));

        registry.apply_seen("session-1", NOW_MS - 2_000);
        assert!(is_unread(&mut registry, "session-1"));

        registry.apply_seen("session-1", NOW_MS);
        assert!(!is_unread(&mut registry, "session-1"));
    }

    #[test]
    fn a_notification_is_unread_until_a_newer_seen_time_arrives() {
        let (_clock, mut registry) = dated_registry();
        registry.apply_status(
            status(
                "session-1",
                notification("Needs permission"),
                NOW_MS - 1_000,
            ),
            NOW_MS,
        );
        assert!(is_unread(&mut registry, "session-1"));

        registry.apply_seen("session-1", NOW_MS - 2_000);
        assert!(is_unread(&mut registry, "session-1"));

        registry.apply_seen("session-1", NOW_MS);
        assert!(!is_unread(&mut registry, "session-1"));
    }

    #[test]
    fn an_older_seen_time_does_not_replace_a_newer_one() {
        let (_clock, mut registry) = dated_registry();
        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 1_000),
            NOW_MS,
        );
        registry.apply_seen("session-1", NOW_MS);

        registry.apply_seen("session-1", NOW_MS - 5_000);

        assert!(!is_unread(&mut registry, "session-1"));
    }

    #[test]
    fn seeing_an_unknown_session_does_nothing() {
        let (_clock, mut registry) = dated_registry();

        registry.apply_seen("session-never-seen", NOW_MS);

        assert_eq!(registry.snapshot().session_count, 0);
    }

    #[test]
    fn a_session_start_or_a_prompt_is_never_unread() {
        let (_clock, mut registry) = dated_registry();

        registry.apply_status(
            status("session-start", HookEventKind::SessionStart, NOW_MS - 1_000),
            NOW_MS,
        );
        registry.apply_status(
            status(
                "session-prompt",
                HookEventKind::UserPromptSubmit,
                NOW_MS - 1_000,
            ),
            NOW_MS,
        );

        assert!(!is_unread(&mut registry, "session-start"));
        assert!(!is_unread(&mut registry, "session-prompt"));
    }

    /// The rule the VS Code extension applies: only the last event counts, so a
    /// prompt withdraws attention with no seen file at all.
    #[test]
    fn only_the_last_event_decides_unread_as_in_the_extension() {
        let (_clock, mut registry) = dated_registry();
        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 3_000),
            NOW_MS,
        );

        registry.apply_status(
            status("session-1", HookEventKind::UserPromptSubmit, NOW_MS - 2_000),
            NOW_MS,
        );
        assert!(!is_unread(&mut registry, "session-1"));

        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 1_000),
            NOW_MS,
        );
        assert!(is_unread(&mut registry, "session-1"));

        registry.apply_status(
            status("session-1", HookEventKind::SessionStart, NOW_MS),
            NOW_MS,
        );
        assert!(!is_unread(&mut registry, "session-1"));
    }

    #[test]
    fn marking_read_clears_unread_until_a_later_stop() {
        let (_clock, mut registry) = dated_registry();
        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 1_000),
            NOW_MS,
        );

        registry.mark_read("session-1", NOW_MS);
        assert!(!is_unread(&mut registry, "session-1"));

        registry.apply_status(
            status("session-1", HookEventKind::UserPromptSubmit, NOW_MS + 1_000),
            NOW_MS + 1_000,
        );
        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS + 2_000),
            NOW_MS + 2_000,
        );
        assert!(is_unread(&mut registry, "session-1"));
    }

    /// Hook events arriving the old way no longer raise unread: the status
    /// records are its only source.
    #[test]
    fn a_hook_event_does_not_raise_unread() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        assert!(!is_unread(&mut registry, "session-1"));
    }

    #[test]
    fn unread_stands_through_scans_enrichment_resumes_and_ticks() {
        let (clock, mut registry) = dated_registry();
        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 1_000),
            NOW_MS,
        );

        clock.advance(minutes(30));
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1").with_status(SessionStatus::Done)],
            gone: Vec::new(),
        });
        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                label: Some("Review the mock data".to_string()),
                ..SessionEnrichment::default()
            },
        );
        registry.apply_resume("session-1");
        registry.tick();

        assert!(is_unread(&mut registry, "session-1"));
    }

    #[test]
    fn the_snapshot_counts_tracked_and_unread_sessions() {
        let (_clock, mut registry) = dated_registry();

        registry.apply_status(
            status("session-1", HookEventKind::Stop, NOW_MS - 1_000),
            NOW_MS,
        );
        registry.apply_status(
            status(
                "session-2",
                notification("Needs permission"),
                NOW_MS - 1_000,
            ),
            NOW_MS,
        );
        registry.apply_status(
            status("session-3", HookEventKind::UserPromptSubmit, NOW_MS - 1_000),
            NOW_MS,
        );
        registry.apply_seen("session-2", NOW_MS);

        let snapshot = registry.snapshot();

        assert_eq!(snapshot.session_count, 3);
        assert_eq!(snapshot.unread_count, 1);
    }

    /// Records are placed at the time they were written, not the order they
    /// were read in.
    #[test]
    fn sessions_are_ordered_by_when_their_records_were_written() {
        let (_clock, mut registry) = dated_registry();

        registry.apply_status(
            status("session-newer", HookEventKind::Stop, NOW_MS - HOUR_MS),
            NOW_MS,
        );
        registry.apply_status(
            status("session-older", HookEventKind::Stop, NOW_MS - 2 * HOUR_MS),
            NOW_MS,
        );
        enrich(&mut registry, "session-newer", "aydy", "main");
        enrich(&mut registry, "session-older", "aydy", "main");

        assert_eq!(
            ids(&registry.snapshot()),
            ["session-newer", "session-older"]
        );
    }

    #[test]
    fn a_turn_recorded_ten_minutes_ago_has_ten_minutes_of_working_time() {
        let (_clock, mut registry) = dated_registry();

        registry.apply_status(
            status(
                "session-1",
                HookEventKind::UserPromptSubmit,
                NOW_MS - 10 * 60 * 1000,
            ),
            NOW_MS,
        );

        assert_eq!(
            find(&registry.snapshot(), "session-1").working_time,
            minutes(10)
        );
    }

    /// A clock that has run ahead of this machine's says nothing about when
    /// the event happened, so it lands at now rather than in the future.
    #[test]
    fn a_record_dated_in_the_future_lands_at_now() {
        let (_clock, mut registry) = dated_registry();

        registry.apply_status(
            status(
                "session-1",
                HookEventKind::UserPromptSubmit,
                NOW_MS + HOUR_MS,
            ),
            NOW_MS,
        );

        assert_eq!(
            find(&registry.snapshot(), "session-1").working_time,
            Duration::ZERO
        );
    }

    #[test]
    fn sessions_in_a_group_are_ordered_needs_input_working_done_unknown() {
        let (clock, mut registry) = registry();

        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-unknown").jumpable()],
            gone: Vec::new(),
        });
        enrich(&mut registry, "session-unknown", "aydy", "main");

        for (session_id, kind) in [
            ("session-done", HookEventKind::Stop),
            ("session-working", HookEventKind::UserPromptSubmit),
            ("session-needs-input", HookEventKind::UserPromptSubmit),
        ] {
            clock.advance(Duration::from_secs(1));
            registry.apply_hook_event(hook(session_id, kind));
            enrich(&mut registry, session_id, "aydy", "main");
        }

        clock.advance(Duration::from_secs(1));
        registry.apply_hook_event(hook(
            "session-needs-input",
            notification("Needs permission"),
        ));

        let snapshot = registry.snapshot();

        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(
            statuses(&snapshot),
            [
                SessionStatus::NeedsInput,
                SessionStatus::Working,
                SessionStatus::Done,
                SessionStatus::Unknown,
            ]
        );
    }

    #[test]
    fn sessions_with_the_same_status_are_ordered_most_recent_event_first() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-older", HookEventKind::Stop));
        enrich(&mut registry, "session-older", "aydy", "main");
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-newer", HookEventKind::Stop));
        enrich(&mut registry, "session-newer", "aydy", "main");

        assert_eq!(
            ids(&registry.snapshot()),
            ["session-newer", "session-older"]
        );
    }

    #[test]
    fn groups_are_ordered_by_their_most_recent_event() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-older", HookEventKind::Stop));
        enrich(&mut registry, "session-older", "okven", "main");
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-newer", HookEventKind::Stop));
        enrich(&mut registry, "session-newer", "aydy", "main");

        let snapshot = registry.snapshot();
        let repos: Vec<Option<&str>> = snapshot
            .groups
            .iter()
            .map(|group| group.repo.as_deref())
            .collect();

        assert_eq!(repos, [Some("aydy"), Some("okven")]);
    }

    #[test]
    fn the_not_yet_identified_group_comes_after_the_identified_ones() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-unidentified", HookEventKind::Stop));
        registry.apply_enrichment(
            "session-unidentified",
            SessionEnrichment {
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-identified", HookEventKind::Stop));
        enrich(&mut registry, "session-identified", "aydy", "main");

        let snapshot = registry.snapshot();
        let kinds: Vec<SessionGroupKind> = snapshot.groups.iter().map(|group| group.kind).collect();

        assert_eq!(
            kinds,
            [SessionGroupKind::Project, SessionGroupKind::Unidentified]
        );
    }

    #[test]
    fn the_not_jumpable_group_comes_last() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-not-jumpable", HookEventKind::Stop));
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-unidentified", HookEventKind::Stop));
        registry.apply_enrichment(
            "session-unidentified",
            SessionEnrichment {
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-identified", HookEventKind::Stop));
        enrich(&mut registry, "session-identified", "aydy", "main");

        let snapshot = registry.snapshot();
        let kinds: Vec<SessionGroupKind> = snapshot.groups.iter().map(|group| group.kind).collect();

        assert_eq!(
            kinds,
            [
                SessionGroupKind::Project,
                SessionGroupKind::Unidentified,
                SessionGroupKind::NotJumpable,
            ]
        );
    }

    #[test]
    fn the_not_yet_identified_group_comes_after_identified_ones_even_when_most_recent() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-identified", HookEventKind::Stop));
        enrich(&mut registry, "session-identified", "aydy", "main");
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-unidentified", HookEventKind::Stop));
        registry.apply_enrichment(
            "session-unidentified",
            SessionEnrichment {
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );

        let snapshot = registry.snapshot();
        let kinds: Vec<SessionGroupKind> = snapshot.groups.iter().map(|group| group.kind).collect();

        assert_eq!(
            kinds,
            [SessionGroupKind::Project, SessionGroupKind::Unidentified]
        );
    }

    #[test]
    fn the_not_jumpable_group_comes_last_even_when_it_is_the_most_recently_active() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-identified", HookEventKind::Stop));
        enrich(&mut registry, "session-identified", "aydy", "main");
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-unidentified", HookEventKind::Stop));
        registry.apply_enrichment(
            "session-unidentified",
            SessionEnrichment {
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );
        clock.advance(minutes(5));
        registry.apply_hook_event(hook("session-not-jumpable", HookEventKind::Stop));

        let snapshot = registry.snapshot();
        let kinds: Vec<SessionGroupKind> = snapshot.groups.iter().map(|group| group.kind).collect();

        assert_eq!(
            kinds,
            [
                SessionGroupKind::Project,
                SessionGroupKind::Unidentified,
                SessionGroupKind::NotJumpable,
            ]
        );
    }

    #[test]
    fn every_not_jumpable_session_lands_in_one_group() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-desktop", HookEventKind::Stop));
        clock.advance(minutes(1));
        registry.apply_hook_event(hook("session-remote", HookEventKind::Stop));

        let snapshot = registry.snapshot();

        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].kind, SessionGroupKind::NotJumpable);
        assert_eq!(snapshot.groups[0].sessions.len(), 2);
    }

    #[test]
    fn a_jumpable_session_with_no_branch_groups_by_repository_alone() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                repo: Some("aydy".to_string()),
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );

        let snapshot = registry.snapshot();

        assert_eq!(snapshot.groups[0].repo.as_deref(), Some("aydy"));
        assert_eq!(snapshot.groups[0].branch, None);
    }

    #[test]
    fn two_branches_of_one_repository_are_separate_groups() {
        let (clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-main", HookEventKind::Stop));
        enrich(&mut registry, "session-main", "aydy", "main");
        clock.advance(minutes(1));
        registry.apply_hook_event(hook("session-feature", HookEventKind::Stop));
        enrich(&mut registry, "session-feature", "aydy", "feat/registry");

        assert_eq!(registry.snapshot().groups.len(), 2);
    }

    #[test]
    fn a_session_moves_into_its_repository_group_as_soon_as_one_is_known() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );

        enrich(&mut registry, "session-1", "aydy", "main");

        assert_eq!(
            registry.snapshot().groups[0].kind,
            SessionGroupKind::Project
        );
    }

    #[test]
    fn a_session_with_no_label_falls_back_to_a_placeholder() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        assert_eq!(
            find(&registry.snapshot(), "session-1").label,
            DEFAULT_SESSION_LABEL
        );
    }

    #[test]
    fn an_enrichment_supplies_the_row_label() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                label: Some("Review attribution mock data".to_string()),
                ..SessionEnrichment::default()
            },
        );

        assert_eq!(
            find(&registry.snapshot(), "session-1").label,
            "Review attribution mock data"
        );
    }

    #[test]
    fn an_enrichment_records_the_linked_worktree_the_session_sits_in() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                repo: Some("okven".to_string()),
                worktree: Some("eric-attribution-2".to_string()),
                ..SessionEnrichment::default()
            },
        );

        let session = registry
            .session("session-1")
            .expect("the enriched session to be tracked");

        assert_eq!(session.repo.as_deref(), Some("okven"));
        assert_eq!(session.worktree.as_deref(), Some("eric-attribution-2"));
    }

    #[test]
    fn an_enrichment_that_supplies_nothing_leaves_known_fields_alone() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        enrich(&mut registry, "session-1", "aydy", "main");

        registry.apply_enrichment("session-1", SessionEnrichment::default());

        let snapshot = registry.snapshot();
        let session = find(&snapshot, "session-1");

        assert_eq!(session.repo.as_deref(), Some("aydy"));
        assert_eq!(session.branch.as_deref(), Some("main"));
    }

    #[test]
    fn enriching_an_unknown_session_id_does_nothing() {
        let (_clock, mut registry) = registry();

        registry.apply_enrichment(
            "session-never-seen",
            SessionEnrichment {
                label: Some("Nothing".to_string()),
                ..SessionEnrichment::default()
            },
        );

        assert_eq!(registry.snapshot().session_count, 0);
    }

    #[test]
    fn a_hook_event_records_the_paths_it_carries() {
        let (_clock, mut registry) = registry();

        registry.apply_hook_event(
            hook("session-1", HookEventKind::SessionStart)
                .with_cwd("/Users/eric/Code/aydy")
                .with_transcript_path("/Users/eric/.claude/projects/aydy/session-1.jsonl"),
        );

        let session = registry.session("session-1").expect("session-1 is tracked");

        assert_eq!(
            session.cwd.as_deref(),
            Some(std::path::Path::new("/Users/eric/Code/aydy"))
        );
        assert_eq!(
            session.transcript_path.as_deref(),
            Some(std::path::Path::new(
                "/Users/eric/.claude/projects/aydy/session-1.jsonl"
            ))
        );
    }

    #[test]
    fn reconciliation_records_the_cwd_and_entrypoint_it_discovers() {
        let (_clock, mut registry) = registry();

        registry.apply_reconciliation(ReconciliationResult {
            present: vec![DiscoveredSession::new("session-1")
                .with_cwd("/Users/eric/Code/aydy")
                .with_entrypoint("claude-vscode")
                .jumpable()],
            gone: Vec::new(),
        });

        let session = registry.session("session-1").expect("session-1 is tracked");

        assert_eq!(session.entrypoint.as_deref(), Some("claude-vscode"));
        assert!(session.jumpable);
    }

    #[test]
    fn an_empty_registry_produces_an_empty_snapshot() {
        let (_clock, mut registry) = registry();

        let snapshot = registry.snapshot();

        assert!(snapshot.groups.is_empty());
        assert_eq!(snapshot.session_count, 0);
        assert_eq!(snapshot.unread_count, 0);
    }
}
