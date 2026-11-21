//! The collection of tracked sessions: event application, unread state,
//! grouping and ordering.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clock::Clock;
use crate::discovery::status_files::StatusRecord;
use crate::discovery::window_map::WindowMap;
use crate::session::event::{
    FocusReport, HookEvent, HookEventKind, ReconciliationResult, SessionEnrichment, SessionId,
    WindowId,
};
use crate::session::session::Session;
use crate::session::status::SessionStatus;
use crate::session::tab_match::{match_tab, TabCandidate, WindowFocus};

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
    /// Whether this is the session the user is currently in. At most one
    /// session in a snapshot carries it.
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
    /// The label each window last reported on its active tab, before any
    /// session was matched to it. A window reporting no Claude tab at all has
    /// no entry.
    ///
    /// The label is kept rather than only the session it resolved to, because
    /// both of the things it is matched against — the session titles, and
    /// which window runs which session — reach the registry from a scan, up to
    /// a reconciliation interval after the report that needs them. A window
    /// reports the moment its tab changes, so a match made only then is
    /// regularly made too early.
    reported_tabs: HashMap<WindowId, String>,
    /// The session owning each window's active tab. A window with no
    /// identified active tab has no entry.
    active_tabs: HashMap<WindowId, SessionId>,
    /// The one window that currently has focus, if any. Holding a single
    /// window here is what guarantees at most one session is the current one.
    focused_window: Option<WindowId>,
}

impl SessionRegistry {
    /// An empty registry reading time from `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            sessions: HashMap::new(),
            reported_tabs: HashMap::new(),
            active_tabs: HashMap::new(),
            focused_window: None,
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

    /// The session the user is currently in — the one owning the active tab of
    /// the focused window.
    pub fn current_session(&self) -> Option<&str> {
        let window = self.focused_window.as_ref()?;

        self.active_tabs.get(window).map(String::as_str)
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
            self.active_tabs.retain(|_, owner| owner != &session_id);
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

        // A title is the other half of what a reported tab label is matched
        // against, and the half that changes under a tab the user is already
        // watching.
        self.refresh_active_tabs();
    }

    /// Records which window is running each session, replacing every
    /// association currently held.
    ///
    /// The map is rebuilt from scratch on every scan and applied whole, so a
    /// window that has stopped registering takes its associations with it
    /// rather than going on owning sessions it can no longer be reached at. A
    /// session the join could not place is left unplaced for the same reason a
    /// scan never guesses: an association is a licence to send a reveal, and a
    /// reveal sent to the wrong window opens a duplicate live session.
    ///
    /// This is deliberately the whole of the extension's effect on a session.
    /// It sets no other field, so grouping is the same list with the extension
    /// installed and without it.
    pub fn apply_window_map(&mut self, windows: &WindowMap) {
        for session in self.sessions.values_mut() {
            session.window = windows.window_of(&session.id);
        }

        // Which sessions a window is running is half of what a reported tab
        // label is matched against, and it has just been replaced whole.
        self.refresh_active_tabs();
    }

    /// The window running `session_id`, when the join placed it in one.
    ///
    /// This is the guard on the reveal command: a session with no window here
    /// is not revealed anywhere.
    pub fn window_of(&self, session_id: &str) -> Option<WindowId> {
        self.sessions
            .get(session_id)
            .and_then(|session| session.window)
    }

    /// Every session running in `window`.
    ///
    /// One window commonly runs several sessions, which is what makes this the
    /// candidate set for deciding which of them owns that window's active tab.
    pub fn sessions_in_window(&self, window: WindowId) -> Vec<&str> {
        let mut sessions: Vec<&str> = self
            .sessions
            .values()
            .filter(|session| session.window == Some(window))
            .map(|session| session.id.as_str())
            .collect();
        sessions.sort_unstable();

        sessions
    }

    /// Applies what the companion extension reports about one window, working
    /// out which session its active tab is showing.
    ///
    /// The label is kept as reported, so that the match behind it can be made
    /// again every time what it reads changes.
    pub fn apply_window_focus(&mut self, focus: WindowFocus) {
        let WindowFocus {
            window_id,
            focused,
            active_tab_label,
        } = focus;

        match active_tab_label {
            Some(label) => {
                self.reported_tabs.insert(window_id, label);
            }
            // Nothing Claude-shaped is in front of this window any more, so
            // the label it reported before says nothing about it and must not
            // be matched again.
            None => {
                self.reported_tabs.remove(&window_id);
            }
        }

        let active_session = self.identify_tab(window_id);

        self.apply_focus_report(FocusReport::new(window_id, focused, active_session));
    }

    /// The session showing on the tab `window` last reported.
    ///
    /// The candidates are the window's own sessions and nothing else. Titles
    /// repeat across windows — two branches of the same work commonly produce
    /// the same summary — and a tab is only ever showing a session that window
    /// is running, so scoping the match here is what stops one window's tab
    /// naming another window's session.
    fn identify_tab(&self, window: WindowId) -> Option<SessionId> {
        let candidates: Vec<TabCandidate<'_>> = self
            .sessions
            .values()
            .filter(|session| session.window == Some(window))
            .map(|session| TabCandidate {
                session_id: session.id.as_str(),
                label: session.label.as_deref(),
            })
            .collect();

        match_tab(
            self.reported_tabs.get(&window).map(String::as_str),
            &candidates,
        )
        .session()
    }

    /// Matches every reported tab again, because what those matches read has
    /// just changed.
    ///
    /// The extension reports on change and on nothing else, so a report is
    /// matched against whatever AYDY happened to hold at the instant it
    /// arrived. A tab title changes the moment the session is titled, which is
    /// up to a scan before AYDY has read that title — and the user goes on
    /// watching the same tab, so no further report is coming. Leaving that
    /// first match as the answer leaves the session in front of the user not
    /// marked as the current one.
    fn refresh_active_tabs(&mut self) {
        let windows: Vec<WindowId> = self.reported_tabs.keys().copied().collect();

        for window in windows {
            // The focus each window is already held to have: this re-reads the
            // labels, and says nothing new about where the user is.
            let focused = self.focused_window == Some(window);
            let active_session = self.identify_tab(window);

            self.apply_focus_report(FocusReport::new(window, focused, active_session));
        }
    }

    /// Applies one window's focus and active tab, already resolved to a
    /// session.
    ///
    /// A window reporting focus becomes the focused window; the previously
    /// focused one therefore stops being focused, which is what keeps the
    /// current-session mark unique.
    pub fn apply_focus_report(&mut self, report: FocusReport) {
        let FocusReport {
            window_id,
            focused,
            active_session,
        } = report;

        match active_session {
            Some(session_id) => {
                self.active_tabs.insert(window_id, session_id);
            }
            None => {
                self.active_tabs.remove(&window_id);
            }
        }

        if focused {
            self.focused_window = Some(window_id);
        } else if self.focused_window == Some(window_id) {
            self.focused_window = None;
        }
    }

    /// Forgets a window, so a closed window's focus does not persist as the
    /// last known truth and the sessions it was running stop being addressable
    /// through it.
    ///
    /// The next scan would rebuild the associations without it anyway; doing it
    /// here makes a window that has just gone stop owning sessions at once
    /// rather than for the rest of the interval.
    pub fn remove_window(&mut self, window_id: WindowId) {
        self.reported_tabs.remove(&window_id);
        self.active_tabs.remove(&window_id);

        if self.focused_window == Some(window_id) {
            self.focused_window = None;
        }

        for session in self.sessions.values_mut() {
            if session.window == Some(window_id) {
                session.window = None;
            }
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
            self.active_tabs.retain(|_, owner| owner != &session_id);
        }
    }

    /// The grouped, ordered, fully-derived view of every tracked session.
    pub fn snapshot(&mut self) -> RegistrySnapshot {
        self.tick();

        let now = self.clock.now();
        let current = self.current_session().map(str::to_string);

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
                is_current: current.as_deref() == Some(session.id.as_str()),
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
    use std::path::PathBuf;

    use super::*;
    use crate::clock::TestClock;
    use crate::discovery::process_tree::FixedParents;
    use crate::discovery::session_files::{SessionFile, VS_CODE_ENTRYPOINT};
    use crate::discovery::window_map::{associate, WindowMap};
    use crate::session::event::{DiscoveredSession, HookEvent};
    use crate::session::status::UNKNOWN_RESOLUTION_WINDOW;
    use crate::session::tab_match::PLACEHOLDER_TAB_LABEL;

    /// A VS Code window, named the only way one can be: by the process id of
    /// the extension host running in it.
    const WINDOW: WindowId = 65_921;

    /// A second window, for the tests that are about telling two apart.
    const OTHER_WINDOW: WindowId = 17_804;

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

    /// One session file, as the scan hands it to the join.
    fn session_file(pid: u32, session_id: &str) -> SessionFile {
        SessionFile {
            pid,
            session_id: session_id.to_string(),
            cwd: Some(PathBuf::from("/Users/eric/Code/aydy")),
            entrypoint: Some(VS_CODE_ENTRYPOINT.to_string()),
        }
    }

    /// The window map a scan produces for `sessions`, each running under the
    /// window given as its parent.
    fn window_map(sessions: &[(u32, &str, WindowId)]) -> WindowMap {
        let files: Vec<SessionFile> = sessions
            .iter()
            .map(|(pid, session_id, _)| session_file(*pid, session_id))
            .collect();
        let parents = FixedParents::new(
            sessions
                .iter()
                .map(|(pid, _, window)| (*pid, *window))
                .collect::<Vec<(u32, WindowId)>>(),
        );
        let windows: Vec<WindowId> = sessions.iter().map(|(_, _, window)| *window).collect();

        associate(&files, &windows, &parents)
    }

    #[test]
    fn a_session_is_placed_in_the_window_running_it() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_window_map(&window_map(&[(95_646, "session-1", WINDOW)]));

        assert_eq!(registry.window_of("session-1"), Some(WINDOW));
    }

    /// Three sessions in one window is the case this join exists for, not an
    /// edge case.
    #[test]
    fn every_session_running_in_one_window_is_listed_for_it() {
        let (_clock, mut registry) = registry();
        for session_id in ["session-1", "session-2", "session-3"] {
            registry.apply_hook_event(hook(session_id, HookEventKind::Stop));
        }

        registry.apply_window_map(&window_map(&[
            (32_303, "session-1", WINDOW),
            (68_179, "session-2", WINDOW),
            (95_646, "session-3", OTHER_WINDOW),
        ]));

        assert_eq!(
            registry.sessions_in_window(WINDOW),
            vec!["session-1", "session-2"]
        );
        assert_eq!(registry.sessions_in_window(OTHER_WINDOW), vec!["session-3"]);
    }

    #[test]
    fn a_session_the_join_could_not_place_has_no_window() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_window_map(&WindowMap::default());

        assert_eq!(registry.window_of("session-1"), None);
        assert!(registry.sessions_in_window(WINDOW).is_empty());
    }

    /// A window that stops registering stops owning sessions: the next scan
    /// builds a map without it, and applying that map takes the associations
    /// with it. A window that goes on owning sessions attracts a reveal to a
    /// window that is no longer there.
    #[test]
    fn a_scan_that_no_longer_places_a_session_leaves_it_unassociated() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        registry.apply_window_map(&window_map(&[(95_646, "session-1", WINDOW)]));

        registry.apply_window_map(&WindowMap::default());

        assert_eq!(registry.window_of("session-1"), None);
    }

    #[test]
    fn forgetting_a_window_drops_the_sessions_it_was_running() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        registry.apply_hook_event(hook("session-2", HookEventKind::Stop));
        registry.apply_window_map(&window_map(&[
            (95_646, "session-1", WINDOW),
            (27_437, "session-2", OTHER_WINDOW),
        ]));

        registry.remove_window(WINDOW);

        assert_eq!(registry.window_of("session-1"), None);
        assert_eq!(registry.window_of("session-2"), Some(OTHER_WINDOW));
    }

    /// The regression the whole join is fenced off from: uninstalling the
    /// extension must not reshuffle the list. Associations are the only thing
    /// it adds, so the grouped snapshot has to be identical with and without
    /// them.
    ///
    /// The session the join leaves unplaced is what gives the test its teeth.
    /// It is jumpable and in a group with the placed one, so anything that lets
    /// an association reach jumpability moves it into the not-jumpable group
    /// and the two snapshots stop matching.
    #[test]
    fn window_associations_do_not_change_the_grouped_list() {
        let (_clock, mut registry) = registry();
        for session_id in ["session-1", "session-2", "session-3"] {
            registry.apply_hook_event(hook(session_id, HookEventKind::Stop));
        }
        registry.apply_reconciliation(ReconciliationResult {
            present: vec![
                DiscoveredSession::new("session-1").jumpable(),
                DiscoveredSession::new("session-2").jumpable(),
                DiscoveredSession::new("session-3"),
            ],
            gone: Vec::new(),
        });
        for session_id in ["session-1", "session-2"] {
            registry.apply_enrichment(
                session_id,
                SessionEnrichment {
                    repo: Some("aydy".to_string()),
                    ..SessionEnrichment::default()
                },
            );
        }
        let without = registry.snapshot();

        // Only the first session is placed: the other two are exactly the
        // sessions the extension can say nothing about.
        registry.apply_window_map(&window_map(&[(95_646, "session-1", WINDOW)]));

        assert_eq!(registry.window_of("session-1"), Some(WINDOW));
        assert_eq!(registry.snapshot(), without);
    }

    /// Sessions tracked, labelled and placed in the window given for each, the
    /// way a scan and a transcript pass leave them before any tab is reported.
    fn placed(registry: &mut SessionRegistry, sessions: &[(&str, &str, WindowId)]) {
        let mut files: Vec<(u32, &str, WindowId)> = Vec::new();

        for (index, (session_id, label, window)) in sessions.iter().enumerate() {
            registry.apply_hook_event(hook(session_id, HookEventKind::Stop));
            registry.apply_enrichment(
                session_id,
                SessionEnrichment {
                    label: Some((*label).to_string()),
                    ..SessionEnrichment::default()
                },
            );
            files.push((30_000 + index as u32, session_id, *window));
        }

        registry.apply_window_map(&window_map(&files));
    }

    /// A report from `window`, as the endpoint hands one over.
    fn focus(window: WindowId, focused: bool, label: Option<&str>) -> WindowFocus {
        WindowFocus {
            window_id: window,
            focused,
            active_tab_label: label.map(str::to_string),
        }
    }

    #[test]
    fn an_active_tab_label_identifies_the_session_titled_with_it() {
        let (_clock, mut registry) = registry();
        placed(
            &mut registry,
            &[
                ("session-1", "Fix the auth flow", WINDOW),
                ("session-2", "Rewrite the parser", WINDOW),
            ],
        );

        registry.apply_window_focus(focus(WINDOW, true, Some("Fix the auth flow")));

        assert_eq!(registry.current_session(), Some("session-1"));
    }

    /// The truncation is the whole of the match for anything longer than a
    /// couple of words, and a title compared untruncated matches nothing.
    #[test]
    fn an_active_tab_label_identifies_a_session_whose_title_the_editor_cut() {
        let (_clock, mut registry) = registry();
        placed(
            &mut registry,
            &[(
                "session-1",
                "Review attribution mock data test file",
                WINDOW,
            )],
        );

        registry.apply_window_focus(focus(
            WINDOW,
            true,
            Some("Review attribution mock \u{2026}"),
        ));

        assert_eq!(registry.current_session(), Some("session-1"));
    }

    /// Candidates are the reporting window's own sessions. Two branches of one
    /// piece of work produce the same summary often enough that an unscoped
    /// match would name the wrong window's session.
    #[test]
    fn an_active_tab_label_never_identifies_a_session_in_another_window() {
        let (_clock, mut registry) = registry();
        placed(
            &mut registry,
            &[
                ("session-here", "Fix the auth flow", WINDOW),
                ("session-there", "Rewrite the parser", OTHER_WINDOW),
            ],
        );

        registry.apply_window_focus(focus(WINDOW, true, Some("Rewrite the parser")));

        assert_eq!(registry.current_session(), None);
    }

    #[test]
    fn two_sessions_in_one_window_sharing_a_label_leave_the_active_tab_unidentified() {
        let (_clock, mut registry) = registry();
        placed(
            &mut registry,
            &[
                ("session-1", "Fix the auth flow", WINDOW),
                ("session-2", "Fix the auth flow", WINDOW),
            ],
        );

        registry.apply_window_focus(focus(WINDOW, true, Some("Fix the auth flow")));

        assert_eq!(registry.current_session(), None);
    }

    #[test]
    fn the_placeholder_tab_label_leaves_the_active_tab_unidentified() {
        let (_clock, mut registry) = registry();
        placed(
            &mut registry,
            &[("session-1", PLACEHOLDER_TAB_LABEL, WINDOW)],
        );

        registry.apply_window_focus(focus(WINDOW, true, Some(PLACEHOLDER_TAB_LABEL)));

        assert_eq!(registry.current_session(), None);
    }

    /// A session in the sidebar is a view rather than a tab, so a window can
    /// have focus with no Claude tab active at all.
    #[test]
    fn a_window_with_no_claude_tab_active_makes_no_session_current() {
        let (_clock, mut registry) = registry();
        placed(&mut registry, &[("session-1", "Fix the auth flow", WINDOW)]);

        registry.apply_window_focus(focus(WINDOW, true, None));

        assert_eq!(registry.current_session(), None);
    }

    /// A title reaching the registry from the transcript pass, which is a scan
    /// behind the tab that is already showing it.
    fn titled(registry: &mut SessionRegistry, session_id: &str, label: &str) {
        registry.apply_enrichment(
            session_id,
            SessionEnrichment {
                label: Some(label.to_string()),
                ..SessionEnrichment::default()
            },
        );
    }

    /// The tab is renamed the moment the session is titled, and the extension
    /// reports it at once — a scan before AYDY has read that title. The user
    /// goes on watching the same tab, so nothing else is coming to correct it.
    #[test]
    fn an_active_tab_reported_before_its_title_arrives_identifies_its_session_once_it_does() {
        let (_clock, mut registry) = registry();
        placed(&mut registry, &[("session-1", "Fix the auth flow", WINDOW)]);
        registry.apply_window_focus(focus(WINDOW, true, Some("Rewrite the parser")));
        assert_eq!(registry.current_session(), None);

        titled(&mut registry, "session-1", "Rewrite the parser");

        assert_eq!(registry.current_session(), Some("session-1"));
    }

    /// The other half of the match arrives on the same scan: until the join
    /// has placed a session in the reporting window it is not a candidate at
    /// all.
    #[test]
    fn an_active_tab_reported_before_the_join_places_its_session_identifies_it_once_it_does() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        titled(&mut registry, "session-1", "Fix the auth flow");
        registry.apply_window_focus(focus(WINDOW, true, Some("Fix the auth flow")));
        assert_eq!(registry.current_session(), None);

        registry.apply_window_map(&window_map(&[(95_646, "session-1", WINDOW)]));

        assert_eq!(registry.current_session(), Some("session-1"));
    }

    #[test]
    fn a_tab_identified_in_a_background_window_does_not_take_focus_from_the_focused_one() {
        let (_clock, mut registry) = registry();
        placed(
            &mut registry,
            &[
                ("session-here", "Fix the auth flow", WINDOW),
                ("session-there", "Rewrite the parser", OTHER_WINDOW),
            ],
        );
        registry.apply_window_focus(focus(WINDOW, true, Some("Fix the auth flow")));
        registry.apply_window_focus(focus(OTHER_WINDOW, false, Some("Wire the listener")));

        titled(&mut registry, "session-there", "Wire the listener");

        assert_eq!(registry.current_session(), Some("session-here"));
    }

    /// A window that has reported no Claude tab in front is reporting a fact,
    /// not a gap: the label before it is spent and is never matched again.
    #[test]
    fn a_window_that_reported_no_claude_tab_is_not_identified_by_the_label_before_it() {
        let (_clock, mut registry) = registry();
        placed(&mut registry, &[("session-1", "Fix the auth flow", WINDOW)]);
        registry.apply_window_focus(focus(WINDOW, true, Some("Fix the auth flow")));

        registry.apply_window_focus(focus(WINDOW, true, None));
        registry.apply_window_map(&window_map(&[(30_000, "session-1", WINDOW)]));

        assert_eq!(registry.current_session(), None);
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
        titled(&mut registry, "session-1", "Review the mock data");
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
    fn the_active_tab_of_the_focused_window_is_the_current_session() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_focus_report(FocusReport::new(
            WINDOW,
            true,
            Some("session-1".to_string()),
        ));

        assert!(find(&registry.snapshot(), "session-1").is_current);
    }

    #[test]
    fn at_most_one_session_is_the_current_one() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));
        registry.apply_hook_event(hook("session-2", HookEventKind::Stop));

        registry.apply_focus_report(FocusReport::new(
            WINDOW,
            true,
            Some("session-1".to_string()),
        ));
        registry.apply_focus_report(FocusReport::new(
            OTHER_WINDOW,
            true,
            Some("session-2".to_string()),
        ));

        let snapshot = registry.snapshot();
        let current: Vec<&str> = snapshot
            .groups
            .iter()
            .flat_map(|group| &group.sessions)
            .filter(|session| session.is_current)
            .map(|session| session.id.as_str())
            .collect();

        assert_eq!(current, ["session-2"]);
    }

    #[test]
    fn no_session_is_current_when_the_active_tab_belongs_to_an_unfocused_window() {
        let (_clock, mut registry) = registry();
        registry.apply_hook_event(hook("session-1", HookEventKind::Stop));

        registry.apply_focus_report(FocusReport::new(
            WINDOW,
            false,
            Some("session-1".to_string()),
        ));

        assert!(!find(&registry.snapshot(), "session-1").is_current);
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
