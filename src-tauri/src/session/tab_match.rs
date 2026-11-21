//! Which session a VS Code window's active tab is showing.
//!
//! There is no API for this. `vscode.Tab` has no id and no session field, and
//! the Claude Code extension exports nothing, so the only thing a window can
//! say about its active tab is the label the user can see on it. That label is
//! the session's summary, cut the way the editor cuts it, or the placeholder
//! shown while there is no summary to cut.
//!
//! The match is made here rather than in the extension because this is the only
//! side that holds the session titles and knows which window is running which
//! session. A window sees itself and nothing else.
//!
//! **A summary is not always a title, so many tabs go unmatched.** The editor
//! takes the summary from the session's first prompt the moment it is sent and
//! keeps it until a generated title replaces it, so a tab commonly reads
//! something AYDY has no record of — AYDY's titles come from the transcript's
//! `ai-title` entries and nothing else. A session the user renamed by hand is
//! worse: the rename is written as `custom-title`, which the transcript reader
//! never looks at, so that tab can never be matched at all. Each of these
//! fails in the safe direction below; together they are why the tab in front
//! of the user is identified less often than it looks like it should be.
//!
//! **An unidentified tab is the answer, not a guess.** Two sessions in one
//! window that have yet to be prompted both read `Claude Code`, and picking one
//! would clear an unread the user never saw — information lost silently.
//! Leaving the tab unidentified suppresses nothing and clears nothing, so the
//! user sees an unread they have to click. That is the recoverable failure of
//! the two.

use crate::session::event::{SessionId, WindowId};

/// The label a Claude Code tab carries while its session has no summary yet.
///
/// It is a placeholder, not a name: every session that has not been prompted
/// yet, in every window, shows exactly this, so it identifies nothing.
pub const PLACEHOLDER_TAB_LABEL: &str = "Claude Code";

/// How much of a summary survives when the editor cuts one, counted the way
/// the editor counts.
///
/// The editor measures and cuts a JavaScript string, so its unit is the UTF-16
/// code unit and not the character: an emoji costs two of these. Counting
/// characters instead agrees for everything on the basic plane and quietly
/// disagrees for a title with an emoji in it, which is a label the window will
/// not recognise and so a tab that never comes forward.
const TAB_LABEL_KEPT: usize = 24;

/// The longest summary the editor shows whole, in those same units.
///
/// One longer than what a cut label keeps, so a summary of exactly this length
/// appears in full rather than as twenty-four units and an ellipsis. Comparing
/// against the wrong one of the two never matches.
const TAB_LABEL_WHOLE: usize = 25;

/// The mark the editor appends to a summary it cut.
const TRUNCATION_MARK: char = '…';

/// What one VS Code window reports about itself, before the active tab has been
/// matched to a session.
///
/// Focus and the active tab stay separate all the way through. A tab active in
/// a window sitting in the background is precisely the walked-away case the app
/// exists to notify about, so the two facts are never combined into one flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowFocus {
    pub window_id: WindowId,
    /// Whether this window currently has focus.
    pub focused: bool,
    /// The label on this window's active Claude Code tab, or `None` when no
    /// Claude Code tab is active in it — a session hosted in the sidebar is a
    /// view rather than a tab, and never appears among a window's tabs at all.
    pub active_tab_label: Option<String>,
}

/// A session the reporting window could be showing, and the label AYDY holds
/// for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabCandidate<'a> {
    pub session_id: &'a str,
    /// The row label AYDY shows for the session.
    ///
    /// It is the session's title whenever there is one, and a branch or a
    /// directory name when there is not — and a session with no title is
    /// showing its first prompt on its tab, or the placeholder until it has
    /// been prompted. Neither of those is ever a branch or a directory name, so
    /// a fallback label can only ever fail to match, which is the failure this
    /// module prefers.
    pub label: Option<&'a str>,
}

/// What a window's active tab turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveTab {
    /// No Claude Code tab is active in the window.
    None,
    /// A Claude Code tab is active, but nothing says which session it shows.
    Unidentified,
    /// The session the active tab is showing.
    Session(SessionId),
}

impl ActiveTab {
    /// The session the tab is showing, when the label identified one.
    ///
    /// Both of the other answers come back as `None`, because neither licenses
    /// suppressing an unread — they differ in what they say about the window,
    /// not in what may be done with them.
    pub fn session(self) -> Option<SessionId> {
        match self {
            Self::Session(session_id) => Some(session_id),
            Self::None | Self::Unidentified => None,
        }
    }
}

/// The label the editor puts on a tab showing a session summarised as `title`.
///
/// A title has to be cut the same way before it is compared, or nothing longer
/// than a couple of words ever matches.
pub fn tab_label(title: &str) -> String {
    if title.encode_utf16().count() <= TAB_LABEL_WHOLE {
        return title.to_string();
    }

    let mut label = String::new();
    let mut kept = 0;

    for character in title.chars() {
        let width = character.len_utf16();

        // A character the editor counts twice, straddling the cut, is left out
        // rather than halved. The editor keeps the first half of it — half of
        // a surrogate pair, which is not text this side can hold at all — so
        // there is no label to match there, and one character short is the
        // answer that at worst fails to match rather than matching the wrong
        // tab.
        if kept + width > TAB_LABEL_KEPT {
            break;
        }

        label.push(character);
        kept += width;
    }

    label.push(TRUNCATION_MARK);

    label
}

/// The session behind `label`, out of the sessions running in the window that
/// reported it.
///
/// Only an unambiguous match identifies a session. A label that matches nothing
/// and a label two candidates both answer to are both unidentified, because
/// the cost of the two failures is not the same: an unidentified tab leaves an
/// unread for the user to click, and a wrong one clears an unread they never
/// saw.
pub fn match_tab(label: Option<&str>, candidates: &[TabCandidate]) -> ActiveTab {
    let Some(label) = label else {
        return ActiveTab::None;
    };

    let label = label.trim();

    if label.is_empty() || label == PLACEHOLDER_TAB_LABEL {
        return ActiveTab::Unidentified;
    }

    let mut matched: Option<&str> = None;

    for candidate in candidates {
        let Some(title) = candidate.label else {
            continue;
        };

        if tab_label(title.trim()) != label {
            continue;
        }

        if matched.is_some() {
            return ActiveTab::Unidentified;
        }

        matched = Some(candidate.session_id);
    }

    match matched {
        Some(session_id) => ActiveTab::Session(session_id.to_string()),
        None => ActiveTab::Unidentified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A title long enough that the editor cuts it, and its cut form.
    const LONG_TITLE: &str = "Review attribution mock data test file";
    const LONG_LABEL: &str = "Review attribution mock \u{2026}";

    fn candidate<'a>(session_id: &'a str, label: &'a str) -> TabCandidate<'a> {
        TabCandidate {
            session_id,
            label: Some(label),
        }
    }

    #[test]
    fn a_title_short_enough_to_show_whole_is_its_own_tab_label() {
        assert_eq!(tab_label("Fix the auth flow"), "Fix the auth flow");
    }

    #[test]
    fn a_title_of_exactly_twenty_five_characters_is_shown_whole() {
        let title = "abcdefghijklmnopqrstuvwxy";

        assert_eq!(title.chars().count(), 25);
        assert_eq!(tab_label(title), title);
    }

    #[test]
    fn a_title_of_twenty_six_characters_is_cut_to_twenty_four_and_an_ellipsis() {
        let title = "abcdefghijklmnopqrstuvwxyz";

        assert_eq!(tab_label(title), "abcdefghijklmnopqrstuvwx\u{2026}");
    }

    #[test]
    fn an_emoji_counts_twice_towards_the_length_the_editor_shows_whole() {
        let title = "Check the release notes \u{1F389}";

        assert_eq!(title.chars().count(), 25);
        assert_eq!(tab_label(title), "Check the release notes \u{2026}");
    }

    #[test]
    fn an_emoji_straddling_the_cut_is_left_out_rather_than_halved() {
        let title = "Twenty three chars here\u{1F389} and more";

        assert_eq!(tab_label(title), "Twenty three chars here\u{2026}");
    }

    #[test]
    fn a_label_matches_the_session_whose_title_it_is() {
        let active = match_tab(
            Some("Fix the auth flow"),
            &[
                candidate("session-1", "Fix the auth flow"),
                candidate("session-2", "Something else"),
            ],
        );

        assert_eq!(active, ActiveTab::Session("session-1".to_string()));
    }

    #[test]
    fn a_label_longer_than_the_editor_shows_matches_its_cut_form() {
        let active = match_tab(Some(LONG_LABEL), &[candidate("session-1", LONG_TITLE)]);

        assert_eq!(active, ActiveTab::Session("session-1".to_string()));
    }

    #[test]
    fn an_untruncated_title_never_matches_the_label_the_editor_shows() {
        let active = match_tab(Some(LONG_TITLE), &[candidate("session-1", LONG_TITLE)]);

        assert_eq!(active, ActiveTab::Unidentified);
    }

    #[test]
    fn the_placeholder_label_identifies_no_session() {
        let active = match_tab(
            Some(PLACEHOLDER_TAB_LABEL),
            &[candidate("session-1", PLACEHOLDER_TAB_LABEL)],
        );

        assert_eq!(active, ActiveTab::Unidentified);
    }

    #[test]
    fn two_candidates_with_the_same_label_identify_no_session() {
        let active = match_tab(
            Some("Fix the auth flow"),
            &[
                candidate("session-1", "Fix the auth flow"),
                candidate("session-2", "Fix the auth flow"),
            ],
        );

        assert_eq!(active, ActiveTab::Unidentified);
    }

    #[test]
    fn a_label_no_candidate_answers_to_identifies_no_session() {
        let active = match_tab(
            Some("Fix the auth flow"),
            &[candidate("session-1", "Something else")],
        );

        assert_eq!(active, ActiveTab::Unidentified);
    }

    #[test]
    fn a_window_with_no_claude_tab_active_reports_no_tab_rather_than_an_unidentified_one() {
        let active = match_tab(None, &[candidate("session-1", "Fix the auth flow")]);

        assert_eq!(active, ActiveTab::None);
    }

    #[test]
    fn a_blank_label_identifies_no_session() {
        let active = match_tab(Some("   "), &[candidate("session-1", "Fix the auth flow")]);

        assert_eq!(active, ActiveTab::Unidentified);
    }

    #[test]
    fn a_candidate_with_no_label_at_all_is_never_matched() {
        let active = match_tab(
            Some("Fix the auth flow"),
            &[TabCandidate {
                session_id: "session-1",
                label: None,
            }],
        );

        assert_eq!(active, ActiveTab::Unidentified);
    }

    #[test]
    fn only_an_identified_tab_yields_a_session_to_suppress_with() {
        assert_eq!(
            ActiveTab::Session("session-1".to_string()).session(),
            Some("session-1".to_string())
        );
        assert_eq!(ActiveTab::Unidentified.session(), None);
        assert_eq!(ActiveTab::None.session(), None);
    }
}
