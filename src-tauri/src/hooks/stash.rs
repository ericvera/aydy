//! What AYDY removed from the user's settings, kept so uninstall can put it
//! back.
//!
//! Removing configuration the user did not create and never giving it back
//! leaves them worse off than before they installed, so the removed entries are
//! recorded verbatim — including keys AYDY does not understand, and including
//! the difference between an entry that carries a `matcher` and one that does
//! not.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hooks::HookError;

/// One hook entry AYDY removed, and where it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedEntry {
    /// The event the entry was registered for.
    pub event: String,
    /// Where the enclosing matcher group sat in the event's list.
    pub group_index: usize,
    /// Where the hook sat inside that group.
    pub hook_index: usize,
    /// Whether removing the hook emptied its group, so that the whole group
    /// went with it.
    pub group_removed: bool,
    /// The hook, exactly as it was written.
    pub hook: Value,
    /// The enclosing group, exactly as it was written, so a group that was
    /// removed whole comes back with every key it had.
    pub group: Value,
}

impl RemovedEntry {
    /// The command the removed hook ran, when it had one. This is what names
    /// the entry on the first-run screen.
    pub fn command(&self) -> Option<&str> {
        self.hook.get("command").and_then(Value::as_str)
    }
}

/// The stash file's contents.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StashContents {
    entries: Vec<RemovedEntry>,
}

/// The file the removed entries are kept in.
#[derive(Debug, Clone)]
pub struct HookStash {
    path: PathBuf,
}

impl HookStash {
    /// A stash held at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where the stash lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Everything currently stashed.
    ///
    /// A missing or unreadable stash reads as empty: a stash AYDY cannot parse
    /// must not be able to block an uninstall, which is the action a user takes
    /// when they want AYDY out of their settings file.
    pub fn entries(&self) -> Vec<RemovedEntry> {
        let Ok(raw) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };

        serde_json::from_str::<StashContents>(&raw)
            .map(|contents| contents.entries)
            .unwrap_or_default()
    }

    /// Replaces the stash with `entries`.
    pub fn replace(&self, entries: &[RemovedEntry]) -> Result<(), HookError> {
        let failed = |source| HookError::Write {
            path: self.path.clone(),
            source,
        };

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(failed)?;
        }

        let contents = StashContents {
            entries: entries.to_vec(),
        };
        let text = serde_json::to_string_pretty(&contents).expect("the stash to serialise");

        fs::write(&self.path, text).map_err(failed)
    }

    /// Empties the stash, once everything in it has been restored.
    pub fn clear(&self) -> Result<(), HookError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(HookError::Write {
                path: self.path.clone(),
                source,
            }),
        }
    }
}

/// Adds `removed` to `stashed`, skipping entries already recorded.
///
/// Installing twice must not stash the same entry twice: an entry recorded but
/// not actually removed would be restored as a duplicate.
///
/// **The identity is the position as well as the body.** Two structurally
/// identical hooks are a plausible configuration — a different sound per
/// notification kind is written as one `afplay` per matcher group, and the two
/// commands are then the same string. Comparing bare bodies makes the second
/// one look like a re-stash of the first, so it is dropped: removed from the
/// user's file, never recorded, and gone for good. `group_index` and
/// `hook_index` are what tell a genuine re-stash (same entry, same place) from
/// a second entry that merely reads the same.
pub fn merge_stashed(stashed: Vec<RemovedEntry>, removed: Vec<RemovedEntry>) -> Vec<RemovedEntry> {
    let mut merged = stashed;

    for entry in removed {
        let already = merged.iter().any(|known| {
            known.event == entry.event
                && known.group_index == entry.group_index
                && known.hook_index == entry.hook_index
                && known.hook == entry.hook
        });

        if !already {
            merged.push(entry);
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::test_support::scratch_dir;

    fn entry(event: &str, command: &str) -> RemovedEntry {
        entry_at(event, command, 0, 0)
    }

    fn entry_at(event: &str, command: &str, group_index: usize, hook_index: usize) -> RemovedEntry {
        RemovedEntry {
            event: event.to_string(),
            group_index,
            hook_index,
            group_removed: true,
            hook: serde_json::json!({ "type": "command", "command": command }),
            group: serde_json::json!({
                "hooks": [{ "type": "command", "command": command }]
            }),
        }
    }

    fn stash(name: &str) -> HookStash {
        HookStash::new(scratch_dir(name).join("removed-hooks.json"))
    }

    #[test]
    fn nothing_is_stashed_before_the_first_install() {
        assert_eq!(stash("empty").entries(), Vec::new());
    }

    #[test]
    fn a_stashed_entry_survives_being_written_and_read_back() {
        let stash = stash("roundtrip");
        let entries = vec![entry("Stop", "afplay /System/Library/Sounds/Funk.aiff")];

        stash.replace(&entries).expect("the stash to be written");

        assert_eq!(stash.entries(), entries);
    }

    #[test]
    fn a_stash_that_cannot_be_parsed_reads_as_empty() {
        let stash = stash("corrupt");
        stash
            .replace(&[entry("Stop", "afplay /System/Library/Sounds/Funk.aiff")])
            .expect("the stash to be written");
        fs::write(stash.path(), "{ not json").expect("the corruption to land");

        assert_eq!(stash.entries(), Vec::new());
    }

    #[test]
    fn clearing_an_absent_stash_is_not_an_error() {
        stash("clear-absent").clear().expect("clearing to succeed");
    }

    #[test]
    fn clearing_a_stash_empties_it() {
        let stash = stash("clear");
        stash
            .replace(&[entry("Stop", "afplay /System/Library/Sounds/Funk.aiff")])
            .expect("the stash to be written");

        stash.clear().expect("the stash to be cleared");

        assert_eq!(stash.entries(), Vec::new());
    }

    #[test]
    fn merging_keeps_an_entry_that_is_already_stashed_exactly_once() {
        let funk = entry("Stop", "afplay /System/Library/Sounds/Funk.aiff");
        let hero = entry("Notification", "afplay /System/Library/Sounds/Hero.aiff");

        let merged = merge_stashed(vec![funk.clone(), hero.clone()], vec![funk.clone()]);

        assert_eq!(merged, vec![funk, hero]);
    }

    #[test]
    fn merging_adds_an_entry_that_is_not_stashed_yet() {
        let funk = entry("Stop", "afplay /System/Library/Sounds/Funk.aiff");
        let hero = entry("Notification", "afplay /System/Library/Sounds/Hero.aiff");

        let merged = merge_stashed(vec![funk.clone()], vec![hero.clone()]);

        assert_eq!(merged, vec![funk, hero]);
    }

    #[test]
    fn two_identical_hooks_removed_from_different_groups_are_both_recorded() {
        let hero = "afplay /System/Library/Sounds/Hero.aiff";
        let first = entry_at("Notification", hero, 0, 0);
        let second = entry_at("Notification", hero, 1, 0);

        let merged = merge_stashed(Vec::new(), vec![first.clone(), second.clone()]);

        assert_eq!(merged, vec![first, second]);
    }

    #[test]
    fn two_identical_hooks_removed_from_one_group_are_both_recorded() {
        let hero = "afplay /System/Library/Sounds/Hero.aiff";
        let first = entry_at("Notification", hero, 0, 0);
        let second = entry_at("Notification", hero, 0, 1);

        let merged = merge_stashed(Vec::new(), vec![first.clone(), second.clone()]);

        assert_eq!(merged, vec![first, second]);
    }

    #[test]
    fn two_identical_hooks_under_different_events_are_both_recorded() {
        let hero = "afplay /System/Library/Sounds/Hero.aiff";
        let stop = entry("Stop", hero);
        let notification = entry("Notification", hero);

        let merged = merge_stashed(Vec::new(), vec![stop.clone(), notification.clone()]);

        assert_eq!(merged, vec![stop, notification]);
    }
}
