//! Locating, reading, and atomically writing `~/.claude/settings.json`.
//!
//! The document is held as a [`serde_json::Value`], never as a typed model of
//! "the settings AYDY knows about": deserializing into a struct and
//! re-serializing deletes every key the struct does not model, which on this
//! file would take the user's permissions, plugins and model settings with it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::atomic_file;
use crate::hooks::HookError;

/// The directory Claude Code keeps its user settings in, unless
/// `CLAUDE_CONFIG_DIR` says otherwise.
const CONFIG_DIR: &str = ".claude";

/// The environment variable Claude Code reads its configuration directory from.
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// The settings file inside that directory.
const SETTINGS_FILE: &str = "settings.json";

/// The indentation used for a file that has none to copy.
const DEFAULT_INDENT: &str = "  ";

/// How the file on disk is laid out, so a rewrite looks like the file that was
/// read rather than like serde's defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonStyle {
    /// One level of indentation.
    pub indent: String,
    /// Whether the file ends with a newline.
    pub trailing_newline: bool,
}

impl JsonStyle {
    /// The layout of `raw`, taken from its first indented line.
    pub fn detect(raw: &str) -> Self {
        let indent = raw
            .lines()
            .find_map(|line| {
                let content = line.trim_start_matches([' ', '\t']);
                let width = line.len() - content.len();

                (width > 0 && !content.is_empty()).then(|| line[..width].to_string())
            })
            .unwrap_or_else(|| DEFAULT_INDENT.to_string());

        Self {
            indent,
            trailing_newline: raw.ends_with('\n'),
        }
    }
}

impl Default for JsonStyle {
    fn default() -> Self {
        Self {
            indent: DEFAULT_INDENT.to_string(),
            trailing_newline: true,
        }
    }
}

/// Renders `value` the way the file it came from was written.
pub fn render(value: &Value, style: &JsonStyle) -> String {
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(style.indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);

    value
        .serialize(&mut serializer)
        .expect("a JSON value to serialise");

    let mut text = String::from_utf8(buffer).expect("serde_json to emit UTF-8");

    if style.trailing_newline {
        text.push('\n');
    }

    text
}

/// The file exactly as it was when it was read.
///
/// The raw text is kept so that the write can prove nothing else touched the
/// file in the meantime. Comparing the text rather than a timestamp is what
/// makes that check exact.
#[derive(Debug, Clone)]
pub struct SettingsSnapshot {
    raw: Option<String>,
    value: Value,
    style: JsonStyle,
}

impl SettingsSnapshot {
    /// The parsed document.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The layout to write the document back in.
    pub fn style(&self) -> &JsonStyle {
        &self.style
    }

    /// The file's text, or `None` when the file does not exist yet.
    pub fn raw(&self) -> Option<&str> {
        self.raw.as_deref()
    }
}

/// The user's Claude Code settings file.
#[derive(Debug, Clone)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    /// The settings file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The signed-in user's settings file, honouring `CLAUDE_CONFIG_DIR` the
    /// same way Claude Code does.
    pub fn user() -> Result<Self, HookError> {
        Ok(Self::new(config_dir()?.join(SETTINGS_FILE)))
    }

    /// Where the file lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and parses the file.
    ///
    /// A file that does not exist reads as an empty document, so a machine with
    /// no user settings yet can still be installed into.
    pub fn read(&self) -> Result<SettingsSnapshot, HookError> {
        let raw = self.read_raw()?;

        let Some(text) = raw else {
            return Ok(SettingsSnapshot {
                raw: None,
                value: Value::Object(serde_json::Map::new()),
                style: JsonStyle::default(),
            });
        };

        let value = if text.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&text).map_err(|source| HookError::Malformed {
                path: self.path.clone(),
                source,
            })?
        };

        if !value.is_object() {
            return Err(HookError::Unexpected {
                path: self.path.clone(),
                at: "the document root".to_string(),
            });
        }

        Ok(SettingsSnapshot {
            style: JsonStyle::detect(&text),
            raw: Some(text),
            value,
        })
    }

    /// Writes `value` over the file, provided it still holds what `snapshot`
    /// read.
    ///
    /// The check is what makes a concurrent edit safe: another process — or the
    /// user in an editor — that wrote the file since it was read is not
    /// overwritten, and the caller is told nothing was modified. The write
    /// itself goes to a temporary file and is renamed into place, so an
    /// interrupted write cannot truncate the user's settings.
    pub fn write(&self, snapshot: &SettingsSnapshot, value: &Value) -> Result<(), HookError> {
        self.verify_unchanged(snapshot)?;

        self.write_atomically(&render(value, &snapshot.style))
    }

    /// Fails with [`HookError::ChangedOnDisk`] if the file no longer holds what
    /// `snapshot` read.
    ///
    /// [`SettingsFile::write`] does this itself. It is exposed so a caller with
    /// more than one file to write can find out before it has written any of
    /// them, and abort having modified nothing.
    pub fn verify_unchanged(&self, snapshot: &SettingsSnapshot) -> Result<(), HookError> {
        if self.read_raw()? != snapshot.raw {
            return Err(HookError::ChangedOnDisk {
                path: self.path.clone(),
            });
        }

        Ok(())
    }

    /// The file's text, or `None` when it does not exist.
    fn read_raw(&self) -> Result<Option<String>, HookError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(HookError::Read {
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn write_atomically(&self, text: &str) -> Result<(), HookError> {
        atomic_file::write_atomically(&self.path, text).map_err(|source| HookError::Write {
            path: self.path.clone(),
            source,
        })
    }
}

/// Claude Code's configuration directory.
fn config_dir() -> Result<PathBuf, HookError> {
    if let Some(configured) = std::env::var_os(CONFIG_DIR_VARIABLE) {
        if !configured.is_empty() {
            return Ok(PathBuf::from(configured));
        }
    }

    let home = std::env::var_os("HOME").filter(|home| !home.is_empty());

    Ok(PathBuf::from(home.ok_or(HookError::NoHome)?).join(CONFIG_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::test_support::{scratch_dir, settings_fixture, REAL_SETTINGS};

    #[test]
    fn a_file_that_does_not_exist_reads_as_an_empty_document() {
        let file = SettingsFile::new(scratch_dir("absent").join("settings.json"));

        let snapshot = file.read().expect("an empty document");

        assert_eq!(snapshot.value(), &serde_json::json!({}));
        assert_eq!(snapshot.raw(), None);
    }

    #[test]
    fn a_settings_file_round_trips_byte_for_byte() {
        let file = SettingsFile::new(settings_fixture("roundtrip", REAL_SETTINGS));
        let snapshot = file.read().expect("the fixture to parse");

        let rendered = render(snapshot.value(), snapshot.style());

        assert_eq!(rendered, REAL_SETTINGS);
    }

    #[test]
    fn a_file_indented_with_four_spaces_is_written_back_with_four() {
        let original = "{\n    \"model\": \"opus\"\n}\n";
        let file = SettingsFile::new(settings_fixture("indent", original));
        let snapshot = file.read().expect("the fixture to parse");

        file.write(&snapshot, snapshot.value())
            .expect("the file to be written");

        assert_eq!(fs::read_to_string(file.path()).unwrap(), original);
    }

    #[test]
    fn a_file_with_no_trailing_newline_is_written_back_without_one() {
        let original = "{\n  \"model\": \"opus\"\n}";
        let file = SettingsFile::new(settings_fixture("no-newline", original));
        let snapshot = file.read().expect("the fixture to parse");

        file.write(&snapshot, snapshot.value())
            .expect("the file to be written");

        assert_eq!(fs::read_to_string(file.path()).unwrap(), original);
    }

    #[test]
    fn a_file_that_is_not_json_is_never_written_over() {
        let file = SettingsFile::new(settings_fixture("malformed", "{ not json at all"));

        let error = file.read().expect_err("a parse failure");

        assert!(matches!(error, HookError::Malformed { .. }));
        assert_eq!(
            fs::read_to_string(file.path()).unwrap(),
            "{ not json at all"
        );
    }

    #[test]
    fn a_json_document_that_is_not_an_object_is_rejected() {
        let file = SettingsFile::new(settings_fixture("array", "[1, 2, 3]\n"));

        let error = file.read().expect_err("a shape failure");

        assert!(matches!(error, HookError::Unexpected { .. }));
    }

    #[test]
    fn a_file_that_changed_since_it_was_read_is_not_overwritten() {
        let file = SettingsFile::new(settings_fixture("changed", REAL_SETTINGS));
        let snapshot = file.read().expect("the fixture to parse");
        let meanwhile = "{\n  \"model\": \"sonnet\"\n}\n";
        fs::write(file.path(), meanwhile).expect("the concurrent write to land");

        let error = file
            .write(&snapshot, &serde_json::json!({ "model": "haiku" }))
            .expect_err("the write to abort");

        assert!(matches!(error, HookError::ChangedOnDisk { .. }));
        assert_eq!(fs::read_to_string(file.path()).unwrap(), meanwhile);
    }

    #[test]
    fn a_file_created_since_it_was_read_is_not_overwritten() {
        let path = scratch_dir("created").join("settings.json");
        let file = SettingsFile::new(&path);
        let snapshot = file.read().expect("an empty document");
        fs::write(&path, REAL_SETTINGS).expect("the concurrent write to land");

        let error = file
            .write(&snapshot, &serde_json::json!({ "model": "haiku" }))
            .expect_err("the write to abort");

        assert!(matches!(error, HookError::ChangedOnDisk { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), REAL_SETTINGS);
    }

    #[test]
    fn writing_leaves_no_temporary_file_behind() {
        let file = SettingsFile::new(settings_fixture("temporary", REAL_SETTINGS));
        let snapshot = file.read().expect("the fixture to parse");

        file.write(&snapshot, snapshot.value())
            .expect("the file to be written");

        let directory = file.path().parent().expect("a parent directory");
        let left: Vec<String> = fs::read_dir(directory)
            .expect("the directory to be readable")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| name != "settings.json")
            .collect();

        assert_eq!(left, Vec::<String>::new());
    }

    #[test]
    fn the_settings_path_follows_the_claude_configuration_directory() {
        let directory = scratch_dir("config-dir");
        temporarily_set(
            &[(CONFIG_DIR_VARIABLE, Some(directory.as_os_str()))],
            || {
                assert_eq!(
                    SettingsFile::user().expect("a settings file").path(),
                    directory.join("settings.json")
                );
            },
        );
    }

    #[test]
    fn the_settings_path_defaults_to_the_claude_directory_in_the_home_directory() {
        let home = scratch_dir("home");
        temporarily_set(
            &[
                (CONFIG_DIR_VARIABLE, None),
                ("HOME", Some(home.as_os_str())),
            ],
            || {
                assert_eq!(
                    SettingsFile::user().expect("a settings file").path(),
                    home.join(".claude").join("settings.json")
                );
            },
        );
    }

    /// Runs `body` with each named variable set, restoring them afterwards.
    ///
    /// Serialised across the whole test binary, because the environment is
    /// process-wide and tests run in parallel. Every variable a test needs goes
    /// in one call: the lock is not re-entrant.
    fn temporarily_set(variables: &[(&str, Option<&std::ffi::OsStr>)], body: impl FnOnce()) {
        use std::sync::Mutex;

        static ENVIRONMENT: Mutex<()> = Mutex::new(());

        let _guard = ENVIRONMENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous: Vec<Option<std::ffi::OsString>> = variables
            .iter()
            .map(|(name, _)| std::env::var_os(name))
            .collect();

        for (name, value) in variables {
            set_environment(name, *value);
        }

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));

        for ((name, _), value) in variables.iter().zip(&previous) {
            set_environment(name, value.as_deref());
        }

        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    fn set_environment(name: &str, value: Option<&std::ffi::OsStr>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}
