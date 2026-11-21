//! The row label, and the chain it falls back through.
//!
//! A row is never blank, so the chain always ends somewhere: the title Claude
//! Code generated, then the branch, then the name of the working directory,
//! then a fixed placeholder.
//!
//! **The directory name here is not the repository name.** A session in
//! `okven.worktrees/eric-attribution-2` falls back to `eric-attribution-2`, not
//! to `okven` — [`repo`] resolves the repository a session is *grouped* by, and
//! a fallback label is instead saying where the session actually is.
//!
//! [`repo`]: crate::discovery::repo

use std::path::Path;

use crate::session::registry::DEFAULT_SESSION_LABEL;

/// The label a session's row shows.
pub fn label(title: Option<&str>, branch: Option<&str>, cwd: Option<&Path>) -> String {
    if let Some(title) = usable(title) {
        return title;
    }
    if let Some(branch) = usable(branch) {
        return branch;
    }
    if let Some(directory) = cwd.and_then(directory_name) {
        return directory;
    }

    DEFAULT_SESSION_LABEL.to_string()
}

/// A candidate with something in it, trimmed.
fn usable(candidate: Option<&str>) -> Option<String> {
    let candidate = candidate?.trim();

    if candidate.is_empty() {
        return None;
    }

    Some(candidate.to_string())
}

/// The last component of a working directory, which is where the session is.
fn directory_name(cwd: &Path) -> Option<String> {
    usable(cwd.file_name()?.to_str())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/Users/eric/Code/okven.worktrees/eric-attribution-2")
    }

    #[test]
    fn the_title_is_the_label_when_there_is_one() {
        let label = label(
            Some("Review attribution mock data test file"),
            Some("eric-attribution-2"),
            Some(&cwd()),
        );

        assert_eq!(label, "Review attribution mock data test file");
    }

    #[test]
    fn a_session_with_no_title_falls_back_to_its_branch() {
        let label = label(None, Some("eric-attribution-2"), Some(&cwd()));

        assert_eq!(label, "eric-attribution-2");
    }

    #[test]
    fn a_session_with_no_title_and_no_branch_falls_back_to_its_directory() {
        let label = label(None, None, Some(&cwd()));

        assert_eq!(label, "eric-attribution-2");
    }

    #[test]
    fn the_fallback_directory_is_the_one_the_session_is_in_not_its_repository() {
        // The repository here is `okven`; the session is not in a directory of
        // that name and the row should not claim it is.
        let label = label(
            None,
            None,
            Some(Path::new("/Users/eric/Code/aydy.worktrees/eric-tray")),
        );

        assert_eq!(label, "eric-tray");
    }

    #[test]
    fn a_session_with_nothing_known_falls_back_to_the_placeholder() {
        let label = label(None, None, None);

        assert_eq!(label, DEFAULT_SESSION_LABEL);
    }

    #[test]
    fn a_working_directory_with_no_last_component_falls_back_to_the_placeholder() {
        let label = label(None, None, Some(Path::new("/")));

        assert_eq!(label, DEFAULT_SESSION_LABEL);
    }

    #[test]
    fn a_blank_title_does_not_win_over_the_branch() {
        let label = label(Some("   "), Some("eric-attribution-2"), Some(&cwd()));

        assert_eq!(label, "eric-attribution-2");
    }

    #[test]
    fn a_blank_branch_does_not_win_over_the_directory() {
        let label = label(None, Some(""), Some(&cwd()));

        assert_eq!(label, "eric-attribution-2");
    }
}
