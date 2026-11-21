//! The label the editor puts on a Claude Code tab.
//!
//! The editor labels a session's tab with its summary, cut to a fixed length
//! and marked with an ellipsis when it is too long to show whole, or with a
//! placeholder while there is no summary to cut. A jump request carries that
//! label so the window can find the tab showing the session.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
