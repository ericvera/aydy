//! AYDY's own settings: what the user chose, what it defaults to, and where it
//! is kept.
//!
//! Everything the user can change lives in one JSON file in the app's config
//! directory.
//!
//! The store owns the rules, not the pane that edits it, so an interface that
//! forgets one cannot put the app into a state that then persists across
//! restarts.

pub mod commands;
pub mod store;

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Anything that can go wrong changing a setting.
#[derive(Debug)]
pub enum SettingsError {
    /// The settings file could not be written. The change is live all the same
    /// — it just may not survive a restart.
    Write { path: PathBuf, source: io::Error },
    /// The login item could not be registered or removed, so launch at login
    /// was left as it was.
    LaunchAtLogin(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write { path, source } => {
                write!(formatter, "could not write {}: {source}", path.display())
            }
            Self::LaunchAtLogin(reason) => {
                write!(formatter, "could not change launch at login: {reason}")
            }
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write { source, .. } => Some(source),
            Self::LaunchAtLogin(_) => None,
        }
    }
}
