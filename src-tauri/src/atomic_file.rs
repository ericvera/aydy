//! Writing a file so that an interrupted write cannot destroy what was there.
//!
//! Every file AYDY owns or edits goes through here: the user's Claude Code
//! settings, and AYDY's own settings. A plain `fs::write` truncates first and
//! fills afterwards, so a crash, a full disk or a power cut between the two
//! leaves an empty or half-written file where the user's configuration was.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// Distinguishes temporary files written at the same moment by the same
/// process, so two threads writing at once cannot land on the same one.
static SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// The name a temporary file falls back to when the target has none.
const FALLBACK_NAME: &str = "file";

/// Writes `contents` over `path`, creating the parent directory if it is
/// missing.
///
/// The bytes go to a temporary file beside the target, are flushed to the
/// device, and the temporary file is then renamed over the target — a rename
/// within one filesystem is atomic, so a reader sees either the old file or the
/// new one and never a partial write. The target's permissions are carried
/// over, so rewriting a private file does not widen it, and a failure at any
/// step takes the temporary file with it.
pub fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    fs::create_dir_all(parent)?;

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(FALLBACK_NAME);
    let temporary = parent.join(format!(
        ".{name}.aydy-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));

    let mut file = fs::File::create(&temporary)?;
    let written = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);

    if let Err(source) = written {
        let _ = fs::remove_file(&temporary);

        return Err(source);
    }

    if let Ok(metadata) = fs::metadata(path) {
        let _ = fs::set_permissions(&temporary, metadata.permissions());
    }

    if let Err(source) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);

        return Err(source);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::scratch_dir;

    /// The names in `directory`, sorted, so a temporary file left behind shows
    /// up as one.
    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("the directory to be readable")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();

        names
    }

    #[test]
    fn a_file_is_written_with_its_contents() {
        let path = scratch_dir("atomic-write").join("settings.json");

        write_atomically(&path, "{\n  \"finishedSound\": false\n}\n").expect("the write to land");

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"finishedSound\": false\n}\n"
        );
    }

    /// A rename leaves nothing beside the file it replaced. A temporary file
    /// left behind would accumulate one per write in a directory the user can
    /// see.
    #[test]
    fn writing_leaves_no_temporary_file_beside_the_target() {
        let directory = scratch_dir("atomic-temporary");
        let path = directory.join("settings.json");

        write_atomically(&path, "first").expect("the first write to land");
        write_atomically(&path, "second").expect("the second write to land");

        assert_eq!(entries(&directory), vec!["settings.json".to_string()]);
    }

    /// The whole point: the bytes never go into the file that is there.
    ///
    /// A hard link is a second name for the same inode, so it keeps whatever
    /// the original file held. A rename gives the target a new inode and leaves
    /// the link on the old contents; a write in place would take the link with
    /// it — and a reader holding the file open at that moment would see a
    /// truncated one.
    #[test]
    fn the_target_is_replaced_rather_than_written_over_in_place() {
        let directory = scratch_dir("atomic-inode");
        let path = directory.join("settings.json");
        let link = directory.join("settings.json.link");
        fs::write(&path, "before").expect("the original to be written");
        fs::hard_link(&path, &link).expect("a second name for the original");

        write_atomically(&path, "after").expect("the write to land");

        assert_eq!(fs::read_to_string(&path).unwrap(), "after");
        assert_eq!(
            fs::read_to_string(&link).unwrap(),
            "before",
            "the new bytes went into the file that was already there"
        );
    }

    #[test]
    fn a_missing_directory_is_created() {
        let path = scratch_dir("atomic-missing")
            .join("nested")
            .join("settings.json");

        write_atomically(&path, "made").expect("the write to land");

        assert_eq!(fs::read_to_string(&path).unwrap(), "made");
    }

    /// A file holding a secret-adjacent configuration must not become
    /// world-readable because it was rewritten. The rename replaces the inode,
    /// so the mode has to be copied deliberately.
    #[test]
    fn the_permissions_of_the_file_being_replaced_are_kept() {
        use std::os::unix::fs::PermissionsExt;

        let path = scratch_dir("atomic-permissions").join("settings.json");
        fs::write(&path, "before").expect("the original to be written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("the mode to be set");

        write_atomically(&path, "after").expect("the write to land");

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    /// The temporary file is named per write, not per target, so two threads
    /// writing the *same* file cannot land on one temporary name — where the
    /// second `File::create` truncates what the first is renaming into place,
    /// and one of the two renames then finds nothing to rename.
    ///
    /// Writing eight different targets would not show this: their temporary
    /// names already differ by the target's own filename.
    #[test]
    fn concurrent_writes_do_not_share_a_temporary_file() {
        let directory = scratch_dir("atomic-concurrent");
        let path = directory.join("settings.json");
        let contents: Vec<String> = (0..8).map(|index| format!("contents {index}")).collect();

        let writers: Vec<_> = contents
            .iter()
            .cloned()
            .map(|contents| {
                let path = path.clone();

                std::thread::spawn(move || {
                    write_atomically(&path, &contents).expect("the write");
                })
            })
            .collect();

        for writer in writers {
            writer.join().expect("the writer to finish");
        }

        let landed = fs::read_to_string(&path).expect("the target to be readable");

        assert!(
            contents.contains(&landed),
            "the file holds neither one write nor another: {landed:?}"
        );
        assert_eq!(entries(&directory), vec!["settings.json".to_string()]);
    }
}
