//! Everything the registry accepts as input.

use std::path::PathBuf;

use crate::extension::registry::ExtensionPid;
use crate::session::status::SessionStatus;

/// A Claude Code session id.
pub type SessionId = String;

/// A VS Code window, named by the process id of the extension host running in
/// it.
///
/// That process id is the window's only identifier, and it is the one name the
/// registration, the session-to-window join and the command channel all use.
/// Naming a window differently in any one of them would leave the three unable
/// to talk about the same window without a conversion that can silently fail to
/// match.
pub type WindowId = ExtensionPid;

/// The five Claude Code hook events the app registers for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEventKind {
    /// A session began.
    SessionStart,
    /// A turn started.
    UserPromptSubmit,
    /// Claude is waiting on the user, with the message describing what is being
    /// awaited when Claude Code supplies one.
    Notification { message: Option<String> },
    /// A turn completed.
    Stop,
    /// The session ended.
    SessionEnd,
}

/// One hook event, as delivered by the listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub session_id: SessionId,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    pub kind: HookEventKind,
}

impl HookEvent {
    /// A hook event for `session_id` carrying no paths.
    pub fn new(session_id: impl Into<SessionId>, kind: HookEventKind) -> Self {
        Self {
            session_id: session_id.into(),
            cwd: None,
            transcript_path: None,
            kind,
        }
    }

    /// Attaches the session's working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Attaches the session's transcript path.
    pub fn with_transcript_path(mut self, transcript_path: impl Into<PathBuf>) -> Self {
        self.transcript_path = Some(transcript_path.into());
        self
    }
}

/// A session a reconciliation scan can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSession {
    pub session_id: SessionId,
    pub cwd: Option<PathBuf>,
    pub entrypoint: Option<String>,
    /// Whether the session belongs to VS Code and lives on this machine.
    pub jumpable: bool,
    /// The status the session's transcript indicates, or `None` when it cannot
    /// be determined. A discovered session is never marked unread.
    pub status: Option<SessionStatus>,
}

impl DiscoveredSession {
    /// A discovered session with no cwd, entrypoint or derived status.
    pub fn new(session_id: impl Into<SessionId>) -> Self {
        Self {
            session_id: session_id.into(),
            cwd: None,
            entrypoint: None,
            jumpable: false,
            status: None,
        }
    }

    /// Attaches the session's working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Attaches the entrypoint reported by the session file.
    pub fn with_entrypoint(mut self, entrypoint: impl Into<String>) -> Self {
        self.entrypoint = Some(entrypoint.into());
        self
    }

    /// Marks the session jumpable.
    pub fn jumpable(mut self) -> Self {
        self.jumpable = true;
        self
    }

    /// Attaches the status derived from the session's transcript.
    pub fn with_status(mut self, status: SessionStatus) -> Self {
        self.status = Some(status);
        self
    }
}

/// The outcome of one reconciliation scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconciliationResult {
    /// The sessions the scan could see.
    pub present: Vec<DiscoveredSession>,
    /// The sessions the scan can positively report gone.
    ///
    /// Only sessions the scan is able to see in the first place can appear
    /// here. Absence from `present` is not evidence of death: the scan never
    /// sees Claude desktop or remote sessions, and treating their absence as
    /// death would silently empty the not-jumpable group.
    pub gone: Vec<SessionId>,
}

/// Per-session fields derived elsewhere and handed to the registry.
///
/// Every field is optional and `None` leaves the current value alone, so an
/// enrichment pass that resolves only a branch does not erase a known label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEnrichment {
    /// The resolved row label. The fallback chain that produces it lives with
    /// transcript enrichment; the registry only guarantees a row is never
    /// blank.
    pub label: Option<String>,
    pub repo: Option<String>,
    /// The linked worktree the session's working directory sits in, when it
    /// sits in one. The repository is what the session is grouped by; this is
    /// the directory detail behind it.
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub jumpable: Option<bool>,
}

/// What the companion extension reports about one editor window.
///
/// Focus and the active tab are two separate facts. A tab active in a
/// background window must not suppress unread, so they are never collapsed
/// into a single flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusReport {
    pub window_id: WindowId,
    /// Whether this window currently has focus.
    pub focused: bool,
    /// The session owning this window's active tab, or `None` when no tab is
    /// active or the active tab could not be matched to a session.
    pub active_session: Option<SessionId>,
}

impl FocusReport {
    /// A report for `window_id` with the given focus and active tab.
    pub fn new(window_id: WindowId, focused: bool, active_session: Option<SessionId>) -> Self {
        Self {
            window_id,
            focused,
            active_session,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hook_event_carries_no_paths_unless_they_are_attached() {
        let event = HookEvent::new("session-1", HookEventKind::Stop);

        assert_eq!(event.cwd, None);
        assert_eq!(event.transcript_path, None);
    }

    #[test]
    fn a_hook_event_carries_the_paths_attached_to_it() {
        let event = HookEvent::new("session-1", HookEventKind::Stop)
            .with_cwd("/Users/eric/Code/aydy")
            .with_transcript_path("/Users/eric/.claude/projects/aydy/session-1.jsonl");

        assert_eq!(event.cwd, Some(PathBuf::from("/Users/eric/Code/aydy")));
        assert_eq!(
            event.transcript_path,
            Some(PathBuf::from(
                "/Users/eric/.claude/projects/aydy/session-1.jsonl"
            ))
        );
    }

    #[test]
    fn a_notification_carries_the_message_describing_what_is_awaited() {
        let event = HookEvent::new(
            "session-1",
            HookEventKind::Notification {
                message: Some("Needs permission for Bash".to_string()),
            },
        );

        assert_eq!(
            event.kind,
            HookEventKind::Notification {
                message: Some("Needs permission for Bash".to_string()),
            }
        );
    }

    #[test]
    fn a_discovered_session_is_not_jumpable_by_default() {
        let discovered = DiscoveredSession::new("session-1");

        assert!(!discovered.jumpable);
        assert_eq!(discovered.status, None);
    }

    #[test]
    fn a_discovered_session_carries_the_details_attached_to_it() {
        let discovered = DiscoveredSession::new("session-1")
            .with_cwd("/Users/eric/Code/aydy")
            .with_entrypoint("claude-vscode")
            .jumpable()
            .with_status(SessionStatus::Done);

        assert_eq!(discovered.cwd, Some(PathBuf::from("/Users/eric/Code/aydy")));
        assert_eq!(discovered.entrypoint, Some("claude-vscode".to_string()));
        assert!(discovered.jumpable);
        assert_eq!(discovered.status, Some(SessionStatus::Done));
    }

    #[test]
    fn an_empty_reconciliation_result_reports_nothing_present_and_nothing_gone() {
        let result = ReconciliationResult::default();

        assert!(result.present.is_empty());
        assert!(result.gone.is_empty());
    }

    #[test]
    fn an_enrichment_leaves_every_field_alone_by_default() {
        let enrichment = SessionEnrichment::default();

        assert_eq!(enrichment.label, None);
        assert_eq!(enrichment.repo, None);
        assert_eq!(enrichment.branch, None);
        assert_eq!(enrichment.jumpable, None);
    }

    #[test]
    fn a_focus_report_keeps_focus_and_the_active_tab_as_separate_facts() {
        let report = FocusReport::new(65_921, false, Some("session-1".to_string()));

        assert!(!report.focused);
        assert_eq!(report.active_session, Some("session-1".to_string()));
    }
}
