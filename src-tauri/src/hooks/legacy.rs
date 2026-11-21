//! Cleaning up after the hook installer older AYDY versions shipped.
//!
//! Those versions registered their own HTTP and curl hooks in the user's Claude
//! settings and stashed the sound hooks they took over in a file of their own.
//! The extension now owns hook installation, so on every start the old entries
//! are removed and the stash is deleted without being restored: the app plays
//! the sounds itself, and restoring them would double every one.

use std::fs;
use std::io;
use std::path::Path;

use crate::hooks::merge::remove_aydy_hooks;
use crate::hooks::settings_file::SettingsFile;
use crate::hooks::HookError;

/// The file the old installer stashed the sound hooks it took over in, inside
/// the app config directory.
pub const LEGACY_STASH_FILE: &str = "removed-hooks.json";

/// Removes every hook entry an older AYDY registered, and reports how many went.
///
/// Nothing is written when there is nothing to remove, so a clean file keeps
/// its timestamp and a concurrent edit is never raced for no reason.
pub fn remove_legacy_hooks(settings: &SettingsFile) -> Result<usize, HookError> {
    let Some(settings) = resolved(settings) else {
        return Ok(0);
    };

    let snapshot = settings.read()?;

    if snapshot.raw().is_none() {
        return Ok(0);
    }

    let mut value = snapshot.value().clone();

    let Some(hooks) = value.get_mut("hooks") else {
        return Ok(0);
    };

    let Some(hooks) = hooks.as_object_mut() else {
        return Err(HookError::Unexpected {
            path: settings.path().to_path_buf(),
            at: "hooks".to_string(),
        });
    };

    let removed = remove_aydy_hooks(hooks);

    if removed == 0 {
        return Ok(0);
    }

    if hooks.is_empty() {
        if let Some(document) = value.as_object_mut() {
            document.shift_remove("hooks");
        }
    }

    settings.write(&snapshot, &value)?;

    Ok(removed)
}

/// Deletes the old installer's stash file, reporting whether there was one.
///
/// The file is never read: the entries in it are not restored.
pub fn remove_legacy_stash(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// The file to read and write in place of `settings`.
///
/// The write renames a temporary file onto the path it is given, which would
/// replace a symlinked settings file (a dotfiles manager's) with a regular one.
/// Writing to the link's real target keeps the link. A dangling link is `None`:
/// its target is missing, so there is nothing to clean.
fn resolved(settings: &SettingsFile) -> Option<SettingsFile> {
    let path = settings.path();
    let is_link = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());

    if !is_link {
        return Some(SettingsFile::new(path));
    }

    fs::canonicalize(path).ok().map(SettingsFile::new)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    use serde_json::Value;

    use super::*;
    use crate::hooks::settings_file::{render, JsonStyle};
    use crate::hooks::test_support::{
        scratch_dir, settings_fixture, REAL_SETTINGS, SETTINGS_WITHOUT_HOOKS,
    };

    fn legacy_http_hook() -> Value {
        serde_json::json!({
            "type": "http",
            "url": "http://127.0.0.1:51234/hook",
            "headers": { "X-Aydy-Hook": "1" },
            "timeout": 2
        })
    }

    fn legacy_curl_hook() -> Value {
        serde_json::json!({
            "type": "command",
            "command": "curl -s -o /dev/null --max-time 1 -H 'Content-Type: application/json' -H 'X-Aydy-Hook: 1' --data-binary @- http://127.0.0.1:51234/hook || true",
            "timeout": 2
        })
    }

    fn extension_hook() -> Value {
        serde_json::json!({
            "type": "command",
            "command": "f=\"$HOME/Library/Application Support/app.aydy/bin/aydy-hook.sh\"; [ -f \"$f\" ] && /bin/sh \"$f\"; exit 0"
        })
    }

    fn real_settings() -> Value {
        serde_json::from_str(REAL_SETTINGS).expect("the fixture to parse")
    }

    fn hooks_of(document: &mut Value) -> &mut serde_json::Map<String, Value> {
        document
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .expect("a hooks object")
    }

    fn push_group(document: &mut Value, event: &str, hook: Value) {
        hooks_of(document)
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("an event list")
            .push(serde_json::json!({ "hooks": [hook] }));
    }

    fn with_legacy_entries(mut document: Value) -> Value {
        push_group(&mut document, "SessionStart", legacy_curl_hook());
        push_group(&mut document, "UserPromptSubmit", legacy_http_hook());
        push_group(&mut document, "Notification", legacy_http_hook());
        push_group(&mut document, "Stop", legacy_http_hook());
        push_group(&mut document, "SessionEnd", legacy_http_hook());

        document
    }

    fn with_extension_entries(mut document: Value) -> Value {
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "Notification",
            "Stop",
            "SessionEnd",
        ] {
            push_group(&mut document, event, extension_hook());
        }

        document
    }

    fn rendered(document: &Value) -> String {
        render(document, &JsonStyle::default())
    }

    fn names_in(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("the directory to be readable")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();

        names
    }

    #[test]
    fn legacy_entries_are_removed_and_everything_else_is_byte_identical() {
        let path = settings_fixture(
            "legacy-only",
            &rendered(&with_legacy_entries(real_settings())),
        );

        let removed = remove_legacy_hooks(&SettingsFile::new(&path)).expect("the cleanup to run");

        assert_eq!(removed, 5);
        assert_eq!(fs::read_to_string(&path).unwrap(), REAL_SETTINGS);
    }

    #[test]
    fn the_extensions_entries_and_the_sound_hooks_are_left_alone() {
        let kept = with_extension_entries(real_settings());
        let path = settings_fixture(
            "legacy-beside-extension",
            &rendered(&with_legacy_entries(kept.clone())),
        );

        let removed = remove_legacy_hooks(&SettingsFile::new(&path)).expect("the cleanup to run");

        assert_eq!(removed, 5);
        assert_eq!(fs::read_to_string(&path).unwrap(), rendered(&kept));
    }

    #[test]
    fn a_hooks_block_left_empty_is_dropped() {
        let mut document: Value =
            serde_json::from_str(SETTINGS_WITHOUT_HOOKS).expect("the fixture to parse");
        document
            .as_object_mut()
            .unwrap()
            .insert("hooks".to_string(), serde_json::json!({}));
        let path = settings_fixture(
            "legacy-empty-hooks",
            &rendered(&with_legacy_entries(document)),
        );

        remove_legacy_hooks(&SettingsFile::new(&path)).expect("the cleanup to run");

        let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(after.get("hooks").is_none(), "{after}");
        assert_eq!(after.get("model"), Some(&Value::from("opus")));
    }

    #[test]
    fn a_file_with_no_legacy_entries_is_not_rewritten() {
        let path = settings_fixture(
            "legacy-none",
            &rendered(&with_extension_entries(real_settings())),
        );
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let removed = remove_legacy_hooks(&SettingsFile::new(&path)).expect("the cleanup to run");

        assert_eq!(removed, 0);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn a_file_with_no_hooks_block_is_not_rewritten() {
        let path = settings_fixture("legacy-no-hooks", SETTINGS_WITHOUT_HOOKS);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let removed = remove_legacy_hooks(&SettingsFile::new(&path)).expect("the cleanup to run");

        assert_eq!(removed, 0);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn a_missing_file_has_nothing_to_remove_and_is_not_created() {
        let path = scratch_dir("legacy-missing").join("settings.json");

        let removed = remove_legacy_hooks(&SettingsFile::new(&path)).expect("the cleanup to run");

        assert_eq!(removed, 0);
        assert!(!path.exists());
    }

    #[test]
    fn malformed_json_is_an_error_and_the_file_is_left_unchanged() {
        let path = settings_fixture("legacy-malformed", "{ not json at all");

        let result = remove_legacy_hooks(&SettingsFile::new(&path));

        assert!(
            matches!(result, Err(HookError::Malformed { .. })),
            "{result:?}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json at all");
    }

    struct LinkedSettings {
        dotfiles: PathBuf,
        claude: PathBuf,
        real: PathBuf,
        link: PathBuf,
    }

    fn linked_settings(name: &str) -> LinkedSettings {
        let root = scratch_dir(name);
        let dotfiles = root.join("dotfiles");
        let claude = root.join("claude");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::create_dir_all(&claude).unwrap();

        LinkedSettings {
            real: dotfiles.join("settings.json"),
            link: claude.join("settings.json"),
            dotfiles,
            claude,
        }
    }

    #[test]
    fn a_symlinked_settings_file_stays_a_symlink_and_its_target_is_cleaned() {
        let linked = linked_settings("legacy-symlink");
        fs::write(
            &linked.real,
            rendered(&with_legacy_entries(real_settings())),
        )
        .unwrap();
        fs::set_permissions(&linked.real, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&linked.real, &linked.link).unwrap();

        let removed =
            remove_legacy_hooks(&SettingsFile::new(&linked.link)).expect("the cleanup to run");

        assert_eq!(removed, 5);
        assert!(fs::symlink_metadata(&linked.link).unwrap().is_symlink());
        assert_eq!(fs::read_link(&linked.link).unwrap(), linked.real);
        assert_eq!(fs::read_to_string(&linked.real).unwrap(), REAL_SETTINGS);
        assert_eq!(
            fs::metadata(&linked.real).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(names_in(&linked.dotfiles), ["settings.json"]);
        assert_eq!(names_in(&linked.claude), ["settings.json"]);
    }

    #[test]
    fn a_dangling_symlink_has_nothing_to_remove_and_creates_nothing() {
        let linked = linked_settings("legacy-dangling");
        symlink(&linked.real, &linked.link).unwrap();

        let removed =
            remove_legacy_hooks(&SettingsFile::new(&linked.link)).expect("the cleanup to run");

        assert_eq!(removed, 0);
        assert!(!linked.real.exists());
        assert!(fs::symlink_metadata(&linked.link).unwrap().is_symlink());
        assert_eq!(names_in(&linked.dotfiles), Vec::<String>::new());
    }

    #[test]
    fn the_stash_is_deleted_without_touching_the_settings_file() {
        let directory = scratch_dir("legacy-stash");
        let stash = directory.join(LEGACY_STASH_FILE);
        let settings = directory.join("settings.json");
        fs::write(&stash, "[]").unwrap();
        fs::write(&settings, REAL_SETTINGS).unwrap();
        let before = fs::metadata(&settings).unwrap().modified().unwrap();

        assert!(remove_legacy_stash(&stash).expect("the stash to be deleted"));
        assert!(!stash.exists());
        assert_eq!(fs::read_to_string(&settings).unwrap(), REAL_SETTINGS);
        assert_eq!(fs::metadata(&settings).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn an_absent_stash_reports_that_there_was_none() {
        let stash = scratch_dir("legacy-no-stash").join(LEGACY_STASH_FILE);

        assert!(!remove_legacy_stash(&stash).expect("an absent stash to be fine"));
    }
}
