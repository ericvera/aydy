//! What the menu bar icon shows, derived from the session snapshot.
//!
//! The derivation lives here rather than in the interface so the icon and the
//! popover are computed from the same snapshot and cannot disagree.

use crate::session::registry::{RegistrySnapshot, SessionSnapshot};
use crate::session::status::SessionStatus;

/// The menu bar states, most urgent first.
///
/// The icons are macOS template images: black plus alpha, tinted by the system
/// for whichever menu bar they land in. Colour therefore carries nothing, and
/// the four states are told apart by shape alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    /// At least one unread session is waiting on the user.
    NeedsInput,
    /// At least one unread session in any other status — the state the app is
    /// named after.
    SomethingNew,
    /// Something is running, and nothing is waiting to be looked at.
    Working,
    /// Nothing needs the user's attention.
    Quiet,
}

impl IconState {
    /// The template image for this state.
    pub fn image(self) -> &'static [u8] {
        match self {
            Self::NeedsInput => include_bytes!("../../icons/tray/needs-input.png"),
            Self::SomethingNew => include_bytes!("../../icons/tray/something-new.png"),
            Self::Working => include_bytes!("../../icons/tray/working.png"),
            Self::Quiet => include_bytes!("../../icons/tray/quiet.png"),
        }
    }
}

/// Everything the menu bar shows: one state, and the count that belongs to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconIndication {
    pub state: IconState,
    /// The number beside the icon. The quiet state carries none.
    pub count: Option<usize>,
}

/// The most urgent state present, with its count.
///
/// Precedence: an unread session that needs input, then an unread session in
/// any other status, then a working session, then nothing. A session that has
/// been read never raises the icon above the working state, which is what makes
/// the icon drop once the user has looked.
pub fn indication(snapshot: &RegistrySnapshot) -> IconIndication {
    if snapshot.unread_count > 0 {
        let waiting = sessions(snapshot)
            .any(|session| session.unread && session.status == SessionStatus::NeedsInput);

        return IconIndication {
            state: if waiting {
                IconState::NeedsInput
            } else {
                IconState::SomethingNew
            },
            count: Some(snapshot.unread_count),
        };
    }

    let working = sessions(snapshot)
        .filter(|session| session.status == SessionStatus::Working)
        .count();

    if working > 0 {
        return IconIndication {
            state: IconState::Working,
            count: Some(working),
        };
    }

    IconIndication {
        state: IconState::Quiet,
        count: None,
    }
}

/// Every session in the snapshot, across all its groups.
fn sessions(snapshot: &RegistrySnapshot) -> impl Iterator<Item = &SessionSnapshot> {
    snapshot.groups.iter().flat_map(|group| &group.sessions)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::session::registry::{SessionGroup, SessionGroupKind};

    /// A session in the given status, read or unread.
    fn session(id: &str, status: SessionStatus, unread: bool) -> SessionSnapshot {
        SessionSnapshot {
            id: id.to_string(),
            label: "A session".to_string(),
            repo: None,
            branch: None,
            status,
            unread,
            jumpable: true,
            working_time: Duration::ZERO,
            longest_turn: Duration::ZERO,
            notification_message: None,
        }
    }

    /// A snapshot holding the given sessions in one group, with the counts the
    /// registry would have derived from them.
    fn snapshot(sessions: Vec<SessionSnapshot>) -> RegistrySnapshot {
        RegistrySnapshot {
            session_count: sessions.len(),
            unread_count: sessions.iter().filter(|session| session.unread).count(),
            groups: vec![SessionGroup {
                kind: SessionGroupKind::Unidentified,
                repo: None,
                branch: None,
                sessions,
            }],
        }
    }

    #[test]
    fn an_empty_registry_leaves_the_menu_bar_quiet() {
        let indication = indication(&snapshot(vec![]));

        assert_eq!(indication.state, IconState::Quiet);
        assert_eq!(indication.count, None);
    }

    #[test]
    fn an_unread_session_that_needs_input_outranks_an_unread_session_that_is_done() {
        let sessions = vec![
            session("a", SessionStatus::Done, true),
            session("b", SessionStatus::NeedsInput, true),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::NeedsInput);
    }

    #[test]
    fn an_unread_session_in_any_other_status_outranks_a_working_session() {
        let sessions = vec![
            session("a", SessionStatus::Done, true),
            session("b", SessionStatus::Working, false),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::SomethingNew);
    }

    #[test]
    fn an_unread_session_that_is_still_unknown_is_something_new() {
        let indication = indication(&snapshot(vec![session("a", SessionStatus::Unknown, true)]));

        assert_eq!(indication.state, IconState::SomethingNew);
    }

    #[test]
    fn a_read_session_that_needs_input_does_not_raise_the_icon_above_working() {
        let sessions = vec![
            session("a", SessionStatus::NeedsInput, false),
            session("b", SessionStatus::Working, false),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::Working);
    }

    #[test]
    fn a_read_session_that_needs_input_does_not_make_another_unread_one_look_like_a_wait() {
        let sessions = vec![
            session("a", SessionStatus::NeedsInput, false),
            session("b", SessionStatus::Done, true),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::SomethingNew);
        assert_eq!(indication.count, Some(1));
    }

    #[test]
    fn a_read_session_that_needs_input_alone_leaves_the_menu_bar_quiet() {
        let indication = indication(&snapshot(vec![session(
            "a",
            SessionStatus::NeedsInput,
            false,
        )]));

        assert_eq!(indication.state, IconState::Quiet);
        assert_eq!(indication.count, None);
    }

    #[test]
    fn the_needs_input_state_counts_every_unread_session_and_not_only_the_waiting_ones() {
        let sessions = vec![
            session("a", SessionStatus::NeedsInput, true),
            session("b", SessionStatus::Done, true),
            session("c", SessionStatus::Working, true),
            session("d", SessionStatus::Done, false),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::NeedsInput);
        assert_eq!(indication.count, Some(3));
    }

    #[test]
    fn the_something_new_state_counts_unread_sessions() {
        let sessions = vec![
            session("a", SessionStatus::Done, true),
            session("b", SessionStatus::Done, true),
            session("c", SessionStatus::Working, false),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::SomethingNew);
        assert_eq!(indication.count, Some(2));
    }

    #[test]
    fn the_working_state_counts_working_sessions_and_not_every_session() {
        let sessions = vec![
            session("a", SessionStatus::Working, false),
            session("b", SessionStatus::Working, false),
            session("c", SessionStatus::Done, false),
            session("d", SessionStatus::NeedsInput, false),
        ];

        let indication = indication(&snapshot(sessions));

        assert_eq!(indication.state, IconState::Working);
        assert_eq!(indication.count, Some(2));
    }

    #[test]
    fn sessions_are_counted_across_every_group() {
        let snapshot = RegistrySnapshot {
            session_count: 2,
            unread_count: 0,
            groups: vec![
                SessionGroup {
                    kind: SessionGroupKind::Project,
                    repo: Some("aydy".to_string()),
                    branch: None,
                    sessions: vec![session("a", SessionStatus::Working, false)],
                },
                SessionGroup {
                    kind: SessionGroupKind::NotJumpable,
                    repo: None,
                    branch: None,
                    sessions: vec![session("b", SessionStatus::Working, false)],
                },
            ],
        };

        let indication = indication(&snapshot);

        assert_eq!(indication.state, IconState::Working);
        assert_eq!(indication.count, Some(2));
    }

    #[test]
    fn an_unread_session_that_needs_input_is_a_wait_whichever_group_it_is_in() {
        let snapshot = RegistrySnapshot {
            session_count: 2,
            unread_count: 1,
            groups: vec![
                SessionGroup {
                    kind: SessionGroupKind::Project,
                    repo: Some("aydy".to_string()),
                    branch: None,
                    sessions: vec![session("a", SessionStatus::Working, false)],
                },
                SessionGroup {
                    kind: SessionGroupKind::NotJumpable,
                    repo: None,
                    branch: None,
                    sessions: vec![session("b", SessionStatus::NeedsInput, true)],
                },
            ],
        };

        let indication = indication(&snapshot);

        assert_eq!(indication.state, IconState::NeedsInput);
        assert_eq!(indication.count, Some(1));
    }

    #[test]
    fn every_state_has_a_template_image_of_its_own() {
        let states = [
            IconState::NeedsInput,
            IconState::SomethingNew,
            IconState::Working,
            IconState::Quiet,
        ];

        for state in states {
            assert!(
                state.image().starts_with(b"\x89PNG\r\n\x1a\n"),
                "{state:?} is not a PNG"
            );
        }

        for (index, state) in states.iter().enumerate() {
            for other in &states[index + 1..] {
                assert_ne!(
                    state.image(),
                    other.image(),
                    "{state:?} and {other:?} share an image"
                );
            }
        }
    }

    /// The four states carry four distinct images, so a pair of them swapped
    /// stays green everywhere except here — and a swap of needs-input and
    /// working would show "still running" while a session is blocked on the
    /// user, which is the one mistake the icon must never make. Each arm is
    /// therefore checked against the file it is named after, read from disk
    /// rather than embedded a second time.
    #[test]
    fn every_state_shows_the_icon_named_after_it() {
        let named = [
            (IconState::NeedsInput, "needs-input.png"),
            (IconState::SomethingNew, "something-new.png"),
            (IconState::Working, "working.png"),
            (IconState::Quiet, "quiet.png"),
        ];

        for (state, name) in named {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("icons/tray")
                .join(name);
            let drawn = fs::read(&path).unwrap_or_else(|error| {
                panic!("expected {} to be readable: {error}", path.display());
            });

            assert_eq!(state.image(), drawn, "{state:?} does not show {name}");
        }
    }
}
