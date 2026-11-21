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
