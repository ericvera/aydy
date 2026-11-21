//! Which VS Code window is running each Claude session.
//!
//! `~/.claude/sessions/<pid>.json` names the Claude process behind a session,
//! and the companion extension registers a window under its extension host's
//! own process id. A session process is a descendant of the extension host of
//! the window it runs in, so matching one process's ancestry against the
//! registered ones joins the two — with no VS Code API, and with no dependency
//! on the Claude Code extension, which exports nothing.
//!
//! **A session nothing places is left unassociated.** Guessing by workspace
//! folder or by recency would be wrong for the case this whole feature exists
//! for — one window running several sessions at once — and being wrong here is
//! not cosmetic: a reveal sent to a window that does not hold the session opens
//! a second live Claude session against the user's work. A session that is not
//! jumpable through the extension is a recoverable disappointment; that is not.
//!
//! **Nothing here takes part in grouping.** Jumpability and the group a session
//! lands in come from its entrypoint and working directory, which the extension
//! has no say in, so installing or removing the extension adds window
//! associations and changes nothing else.

use std::collections::{HashMap, HashSet};

use crate::discovery::process_tree::{ancestors, ProcessParents};
use crate::discovery::session_files::SessionFile;
use crate::session::event::{SessionId, WindowId};

/// The window running each session the join could place.
///
/// A session missing from the map is one nothing placed, which is the ordinary
/// state without the extension installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowMap {
    windows: HashMap<SessionId, WindowId>,
}

impl WindowMap {
    /// The window running `session_id`, or `None` when nothing places it.
    pub fn window_of(&self, session_id: &str) -> Option<WindowId> {
        self.windows.get(session_id).copied()
    }

    /// How many sessions the join placed.
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Whether the join placed nothing at all, which is what a machine with no
    /// extension installed produces on every pass.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

/// Joins each session in `sessions` to whichever of `windows` is running it,
/// resolving ancestry through `parents`.
///
/// The nearest registered ancestor wins, and a session with none is simply
/// absent from the result. No other evidence is consulted: an association is
/// either proven by the process tree or it is not made.
///
/// The map is built from scratch every time rather than amended, which is what
/// makes a closed window's associations go away with its registration — a
/// window that is no longer registered is no longer a candidate, so the
/// sessions it was running come back unplaced.
pub fn associate(
    sessions: &[SessionFile],
    windows: &[WindowId],
    parents: &dyn ProcessParents,
) -> WindowMap {
    let registered: HashSet<WindowId> = windows.iter().copied().collect();

    // Without the extension there is nothing to match against, so no ancestry
    // is read at all.
    if registered.is_empty() {
        return WindowMap::default();
    }

    let windows = sessions
        .iter()
        .filter_map(|session| {
            let window = ancestors(parents, session.pid)
                .into_iter()
                .find(|ancestor| registered.contains(ancestor))?;

            Some((session.session_id.clone(), window))
        })
        .collect();

    WindowMap { windows }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::clock::{Clock, TestClock};
    use crate::discovery::process_tree::FixedParents;
    use crate::discovery::session_files::{self, VS_CODE_ENTRYPOINT};
    use crate::discovery::test_support::sessions_dir;
    use crate::extension::registry::{ExtensionRegistry, Registration, REGISTRATION_TIMEOUT};

    /// The window that ran three sessions at once during planning, which is the
    /// case the whole join exists for.
    const CROWDED_WINDOW: WindowId = 65_921;

    /// A second window, so a test can tell "the right window" from "a window".
    const OTHER_WINDOW: WindowId = 17_804;

    fn session(pid: u32, session_id: &str) -> SessionFile {
        SessionFile {
            pid,
            session_id: session_id.to_string(),
            cwd: Some(PathBuf::from("/Users/eric/Code/aydy")),
            entrypoint: Some(VS_CODE_ENTRYPOINT.to_string()),
        }
    }

    fn window(pid: WindowId) -> Registration {
        Registration {
            pid,
            callback_port: 55_123,
            workspace_folders: vec![PathBuf::from("/Users/eric/Code/aydy")],
            claude_code_version: Some("2.1.220".to_string()),
        }
    }

    fn registered(registry: &ExtensionRegistry) -> Vec<WindowId> {
        registry
            .registrations()
            .into_iter()
            .map(|registration| registration.pid)
            .collect()
    }

    #[test]
    fn a_session_whose_parent_is_a_registered_window_belongs_to_it() {
        let parents = FixedParents::new([(95_646, CROWDED_WINDOW)]);

        let map = associate(&[session(95_646, "session-1")], &[CROWDED_WINDOW], &parents);

        assert_eq!(map.window_of("session-1"), Some(CROWDED_WINDOW));
    }

    /// The case a direct parent check would fail silently: an intermediate
    /// helper between the session and the extension host.
    #[test]
    fn a_session_matched_through_a_grandparent_belongs_to_that_window() {
        let parents = FixedParents::new([(95_646, 95_600), (95_600, CROWDED_WINDOW)]);

        let map = associate(&[session(95_646, "session-1")], &[CROWDED_WINDOW], &parents);

        assert_eq!(map.window_of("session-1"), Some(CROWDED_WINDOW));
    }

    #[test]
    fn a_session_with_no_registered_ancestor_is_left_unassociated() {
        let parents = FixedParents::new([(95_646, 400), (400, 1)]);

        let map = associate(&[session(95_646, "session-1")], &[CROWDED_WINDOW], &parents);

        assert_eq!(map.window_of("session-1"), None);
        assert!(map.is_empty());
    }

    /// A registration carrying a session's own process id is a coincidence, not
    /// evidence that the session is the window running it.
    #[test]
    fn a_session_is_never_matched_to_a_window_registered_under_its_own_process_id() {
        let parents = FixedParents::new([(95_646, 400)]);

        let map = associate(&[session(95_646, "session-1")], &[95_646], &parents);

        assert_eq!(map.window_of("session-1"), None);
    }

    #[test]
    fn three_sessions_sharing_one_extension_host_all_belong_to_the_same_window() {
        let parents = FixedParents::new([
            (32_303, CROWDED_WINDOW),
            (68_179, CROWDED_WINDOW),
            (95_646, CROWDED_WINDOW),
        ]);

        let map = associate(
            &[
                session(32_303, "session-1"),
                session(68_179, "session-2"),
                session(95_646, "session-3"),
            ],
            &[CROWDED_WINDOW],
            &parents,
        );

        assert_eq!(map.window_of("session-1"), Some(CROWDED_WINDOW));
        assert_eq!(map.window_of("session-2"), Some(CROWDED_WINDOW));
        assert_eq!(map.window_of("session-3"), Some(CROWDED_WINDOW));
    }

    #[test]
    fn each_window_gets_the_sessions_running_under_its_own_extension_host() {
        let parents = FixedParents::new([(95_646, CROWDED_WINDOW), (27_437, OTHER_WINDOW)]);

        let map = associate(
            &[session(95_646, "session-1"), session(27_437, "session-2")],
            &[CROWDED_WINDOW, OTHER_WINDOW],
            &parents,
        );

        assert_eq!(map.window_of("session-1"), Some(CROWDED_WINDOW));
        assert_eq!(map.window_of("session-2"), Some(OTHER_WINDOW));
    }

    /// The extension host is the window. The process that started every
    /// extension host on the machine is an ancestor of every session on it, so
    /// when the chain holds more than one registered window the nearer one is
    /// the answer — reaching past it would put every session on the machine
    /// into a single window and send every reveal to the wrong one.
    #[test]
    fn a_session_belongs_to_the_nearest_registered_window_above_it() {
        let parents = FixedParents::new([(95_646, CROWDED_WINDOW), (CROWDED_WINDOW, OTHER_WINDOW)]);

        let map = associate(
            &[session(95_646, "session-1")],
            &[OTHER_WINDOW, CROWDED_WINDOW],
            &parents,
        );

        assert_eq!(map.window_of("session-1"), Some(CROWDED_WINDOW));
    }

    /// The no-extension case: nothing is registered, so nothing is placed and
    /// the app runs exactly as it did before the extension existed.
    #[test]
    fn no_registered_window_places_no_session() {
        let parents = FixedParents::new([(95_646, CROWDED_WINDOW)]);

        let map = associate(&[session(95_646, "session-1")], &[], &parents);

        assert!(map.is_empty());
    }

    #[test]
    fn a_window_whose_registration_has_gone_takes_its_associations_with_it() {
        let clock = Arc::new(TestClock::new());
        let registry = ExtensionRegistry::new(Arc::clone(&clock) as Arc<dyn Clock>);
        let parents = FixedParents::new([(95_646, CROWDED_WINDOW)]);
        let sessions = [session(95_646, "session-1")];
        registry.register(window(CROWDED_WINDOW));

        let placed = associate(&sessions, &registered(&registry), &parents);
        clock.advance(REGISTRATION_TIMEOUT + std::time::Duration::from_secs(1));
        let after = associate(&sessions, &registered(&registry), &parents);

        assert_eq!(placed.window_of("session-1"), Some(CROWDED_WINDOW));
        assert_eq!(after.window_of("session-1"), None);
    }

    /// The session files are undocumented Claude Code internals. A release that
    /// changes their shape costs the associations and nothing else: the join
    /// sees no sessions and returns an empty map rather than failing.
    #[test]
    fn session_files_in_a_shape_that_is_not_recognised_yield_no_associations() {
        let dir = sessions_dir(
            "window-map-unrecognised",
            &[
                (95_646, r#"{"peerProtocol":2,"session":{"id":"session-1"}}"#),
                (32_303, "not json at all"),
            ],
        );
        let parents = FixedParents::new([(95_646, CROWDED_WINDOW), (32_303, CROWDED_WINDOW)]);

        let files = session_files::read_all(&dir).expect("the sessions directory to be readable");
        let map = associate(&files.sessions, &[CROWDED_WINDOW], &parents);

        assert_eq!(files.unrecognised, 2);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }
}
