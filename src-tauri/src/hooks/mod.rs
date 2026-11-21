//! The user's Claude Code settings, and the cleanup of the hook entries older
//! AYDY versions registered in them.
//!
//! The file being edited is `~/.claude/settings.json`, and it belongs to the
//! user, not to AYDY. Everything outside the `hooks` block survives untouched,
//! the document is round-tripped through [`serde_json::Value`] so no key can be
//! dropped by not being modelled, and every write is atomic and aborts if the
//! file changed since it was read.

pub mod legacy;
pub mod merge;
pub mod settings_file;

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Anything that can go wrong reading or writing the files this module owns.
#[derive(Debug)]
pub enum HookError {
    /// The home directory could not be determined, so the settings file cannot
    /// be located.
    NoHome,
    /// A file could not be read.
    Read { path: PathBuf, source: io::Error },
    /// A file could not be written. The settings file is written atomically, so
    /// this never leaves it half-written.
    Write { path: PathBuf, source: io::Error },
    /// The file is not valid JSON. Nothing is written over a file AYDY cannot
    /// parse, because rewriting it would mean guessing at the user's content.
    Malformed {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// The file, or a part of it AYDY has to edit, is not the shape it must be.
    Unexpected { path: PathBuf, at: String },
    /// The file changed on disk between being read and being written. Nothing
    /// was modified.
    ChangedOnDisk { path: PathBuf },
}

impl fmt::Display for HookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => write!(formatter, "could not find the home directory"),
            Self::Read { path, source } => {
                write!(formatter, "could not read {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "could not write {}: {source}", path.display())
            }
            Self::Malformed { path, source } => {
                write!(formatter, "{} is not valid JSON: {source}", path.display())
            }
            Self::Unexpected { path, at } => write!(
                formatter,
                "{} has an unexpected shape at {at}",
                path.display()
            ),
            Self::ChangedOnDisk { path } => write!(
                formatter,
                "{} changed on disk, so nothing was modified",
                path.display()
            ),
        }
    }
}

impl std::error::Error for HookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Malformed { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::PathBuf;

    pub(crate) use crate::test_support::scratch_dir;

    /// The `~/.claude/settings.json` this machine actually has, in the shape
    /// [`crate::hooks::settings_file::render`] produces, so a byte-for-byte
    /// comparison against a re-rendered document is meaningful.
    pub const REAL_SETTINGS: &str = r#"{
  "permissions": {
    "allow": [
      "Bash(git status:*)",
      "Read(//Users/eric/**)"
    ],
    "deny": [
      "Bash(rm -rf:*)"
    ]
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "afplay /System/Library/Sounds/Funk.aiff"
          }
        ]
      }
    ],
    "Notification": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "afplay /System/Library/Sounds/Hero.aiff"
          }
        ]
      }
    ]
  },
  "enabledPlugins": {
    "mise@ericvera": true
  },
  "extraKnownMarketplaces": {
    "ericvera": {
      "source": {
        "source": "github",
        "repo": "ericvera/claude-plugins"
      }
    }
  },
  "effortLevel": "high",
  "tui": {
    "theme": "dark"
  },
  "model": "opus"
}
"#;

    /// The same file with no `hooks` key at all.
    pub const SETTINGS_WITHOUT_HOOKS: &str = r#"{
  "permissions": {
    "allow": [
      "Bash(git status:*)"
    ]
  },
  "model": "opus"
}
"#;

    /// A scratch settings file holding `contents`.
    pub fn settings_fixture(name: &str, contents: &str) -> PathBuf {
        let path = scratch_dir(name).join("settings.json");
        fs::write(&path, contents).expect("the fixture to be written");

        path
    }
}
