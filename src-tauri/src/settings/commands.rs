//! What the settings pane reads and writes.
//!
//! Every command crosses the boundary with the settings as they now are, so the
//! pane renders what the store decided rather than what it asked for — launch
//! at login that the system refused comes back as it was.

use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::settings::store::{
    LoginItem, LoginItemError, Registration, Settings, SettingsChange, SettingsStore,
};
use crate::settings::SettingsError;

/// The settings as the pane renders them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    pub finished_sound: bool,
    pub needs_input_sound: bool,
    pub launch_at_login: bool,
}

impl From<Settings> for SettingsPayload {
    fn from(settings: Settings) -> Self {
        Self {
            finished_sound: settings.finished_sound,
            needs_input_sound: settings.needs_input_sound,
            launch_at_login: settings.launch_at_login,
        }
    }
}

/// Why a settings command could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFailurePayload {
    pub reason: String,
}

impl From<&SettingsError> for SettingsFailurePayload {
    fn from(error: &SettingsError) -> Self {
        let reason = match error {
            SettingsError::Write { .. } => {
                "The change is in force, but AYDY could not save it for next time."
            }
            SettingsError::LaunchAtLogin(_) => {
                "AYDY could not change whether it starts when you log in."
            }
        };

        Self {
            reason: reason.to_string(),
        }
    }
}

/// Launch at login, through the autostart plugin.
///
/// `MacosLauncher::LaunchAgent` writes a plist into `~/Library/LaunchAgents/`.
/// On macOS 13 and later the entry appears in System Settings → Login Items
/// under the **developer or team name**, not under AYDY — which looks like a
/// bug and is not one.
#[derive(Debug)]
pub struct Autostart {
    app: AppHandle,
}

impl Autostart {
    /// The login item for the running app.
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// The launch agent the autostart plugin writes.
    ///
    /// Rebuilt here rather than asked for, because the plugin keeps the path to
    /// itself: it is the launch agent directory in the user's home, holding a
    /// plist named for the product — which is the name the plugin hands the
    /// agent as its label.
    fn agent(&self) -> Result<PathBuf, LoginItemError> {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or_else(|| LoginItemError("there is no home directory".to_string()))?;

        Ok(PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", self.app.package_info().name)))
    }
}

impl LoginItem for Autostart {
    fn registration(&self) -> Result<Registration, LoginItemError> {
        let agent = match fs::read_to_string(self.agent()?) {
            Ok(agent) => agent,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Registration::Absent),
            Err(error) => return Err(LoginItemError(error.to_string())),
        };

        if starts(&agent, &running_executable()?) {
            Ok(Registration::Ours)
        } else {
            Ok(Registration::Elsewhere)
        }
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), LoginItemError> {
        let manager = self.app.autolaunch();
        let changed = if enabled {
            let executable = running_executable()?;

            if !is_bundled(&executable) {
                return Err(LoginItemError(format!(
                    "{} is not an installed copy of AYDY",
                    executable.display()
                )));
            }

            manager.enable()
        } else {
            manager.disable()
        };

        changed.map_err(|error| LoginItemError(error.to_string()))
    }
}

/// The executable this process is running, in the form a login item names it.
///
/// Canonicalised, because that is the form the autostart plugin writes into the
/// agent and the two are only comparable if they are derived the same way.
fn running_executable() -> Result<PathBuf, LoginItemError> {
    std::env::current_exe()
        .and_then(|executable| executable.canonicalize())
        .map_err(|error| LoginItemError(error.to_string()))
}

/// Whether this launch agent starts `executable`.
///
/// The agent is one AYDY wrote through the autostart plugin, whose only strings
/// are the label and the program arguments, so the executable's own path
/// appearing among them is the whole of what "this agent starts it" means.
fn starts(agent: &str, executable: &Path) -> bool {
    agent.contains(&format!("<string>{}</string>", executable.display()))
}

/// Whether `executable` is the one inside a macOS application bundle.
///
/// Only an installed copy has an address worth writing into a login item. A
/// build run straight out of its target directory is deleted by the next clean,
/// and an agent naming it starts nothing at every login afterwards without
/// saying so — so AYDY refuses to register one at all rather than leave the
/// user a login item that reads as set and does nothing.
fn is_bundled(executable: &Path) -> bool {
    executable
        .parent()
        .filter(|directory| directory.file_name() == Some(OsStr::new("MacOS")))
        .and_then(Path::parent)
        .filter(|directory| directory.file_name() == Some(OsStr::new("Contents")))
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .is_some_and(|bundle| bundle.ends_with(".app"))
}

/// The settings, for the pane that has just been opened.
#[tauri::command]
pub async fn read_settings(
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<SettingsPayload, SettingsFailurePayload> {
    Ok(payload(&settings))
}

/// Changes some of the settings, and answers with all of them.
#[tauri::command]
pub async fn update_settings(
    change: SettingsChange,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<SettingsPayload, SettingsFailurePayload> {
    let store = Arc::clone(&settings);

    tauri::async_runtime::spawn_blocking(move || {
        store
            .apply(change)
            .map_err(|error| SettingsFailurePayload::from(&error))?;

        Ok(payload(&store))
    })
    .await
    .map_err(|_| interrupted())?
}

/// The payload for the settings as they stand, reachable without a Tauri app to
/// hold state.
pub fn payload(store: &SettingsStore) -> SettingsPayload {
    SettingsPayload::from(store.current())
}

/// A task that did not finish, which is not something the user can act on.
fn interrupted() -> SettingsFailurePayload {
    SettingsFailurePayload {
        reason: "AYDY could not finish reading your settings.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::settings::store::SETTINGS_FILE;
    use crate::test_support::scratch_dir;

    /// A login item that records nothing, because no test may write a plist
    /// into `~/Library/LaunchAgents/`.
    #[derive(Debug, Default)]
    struct FakeLoginItem;

    impl LoginItem for FakeLoginItem {
        fn registration(&self) -> Result<Registration, LoginItemError> {
            Ok(Registration::Ours)
        }

        fn set_enabled(&self, _enabled: bool) -> Result<(), LoginItemError> {
            Ok(())
        }
    }

    fn store(name: &str) -> SettingsStore {
        SettingsStore::load(
            scratch_dir(name).join(SETTINGS_FILE),
            Arc::new(FakeLoginItem),
        )
    }

    #[test]
    fn the_settings_cross_as_camel_cased_json() {
        let store = store("payload");

        let json = serde_json::to_value(payload(&store)).expect("the payload to serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "finishedSound": true,
                "needsInputSound": true,
                "launchAtLogin": true,
            })
        );
    }

    /// The pane sends only the control that moved.
    #[test]
    fn a_change_arrives_as_camel_cased_json_naming_only_what_moved() {
        let change: SettingsChange =
            serde_json::from_str("{ \"needsInputSound\": false }").expect("a change");

        assert_eq!(
            change,
            SettingsChange {
                needs_input_sound: Some(false),
                ..SettingsChange::default()
            }
        );
    }

    #[test]
    fn every_failure_names_a_reason_of_its_own() {
        let reasons: Vec<String> = [
            SettingsError::Write {
                path: scratch_dir("reasons").join(SETTINGS_FILE),
                source: std::io::Error::other("no"),
            },
            SettingsError::LaunchAtLogin("no".to_string()),
        ]
        .iter()
        .map(|error| SettingsFailurePayload::from(error).reason)
        .collect();

        let mut distinct = reasons.clone();
        distinct.sort();
        distinct.dedup();

        assert_eq!(distinct.len(), reasons.len(), "got {reasons:?}");
        assert!(reasons.iter().all(|reason| reason.ends_with('.')));
    }

    /// The installed copy is the only one whose path stays where it is, so it
    /// is the only one AYDY may name in a login item.
    #[test]
    fn only_an_installed_copy_of_aydy_may_be_a_login_item() {
        assert!(is_bundled(Path::new(
            "/Applications/AYDY.app/Contents/MacOS/aydy"
        )));
        assert!(is_bundled(Path::new(
            "/Users/someone/Applications/AYDY.app/Contents/MacOS/aydy"
        )));
    }

    /// The failure this rule exists for: a login item naming a build directory,
    /// which the next clean empties, leaving every login starting nothing.
    #[test]
    fn a_copy_running_out_of_a_build_directory_may_not_be_a_login_item() {
        assert!(!is_bundled(Path::new(
            "/Users/someone/code/aydy/src-tauri/target/debug/aydy"
        )));
        assert!(!is_bundled(Path::new(
            "/Users/someone/code/aydy/src-tauri/target/release/aydy"
        )));
        assert!(!is_bundled(Path::new("/usr/local/bin/aydy")));
        assert!(!is_bundled(Path::new("aydy")));
    }

    /// A nearby path is not the bundle layout: only the executable macOS starts
    /// counts, not something sitting beside it.
    #[test]
    fn only_the_executable_inside_the_bundle_counts_as_installed() {
        assert!(!is_bundled(Path::new(
            "/Applications/AYDY.app/Contents/aydy"
        )));
        assert!(!is_bundled(Path::new(
            "/Applications/AYDY.app/Contents/Resources/aydy"
        )));
        assert!(!is_bundled(Path::new(
            "/Applications/AYDY/Contents/MacOS/aydy"
        )));
    }

    /// How a launch agent left over from another copy of AYDY is told apart
    /// from this one's, so it can be repointed instead of trusted.
    #[test]
    fn a_launch_agent_is_ours_only_when_it_names_the_running_executable() {
        let agent = agent_for("/Applications/AYDY.app/Contents/MacOS/aydy");

        assert!(starts(
            &agent,
            Path::new("/Applications/AYDY.app/Contents/MacOS/aydy")
        ));
        assert!(!starts(
            &agent,
            Path::new("/Users/someone/code/aydy/src-tauri/target/debug/aydy")
        ));
    }

    /// Two installs whose paths share a prefix are two different copies, and an
    /// agent naming one must not read as the other's — which is what looking for
    /// the whole element rather than the bare path buys.
    #[test]
    fn an_agent_naming_a_longer_path_is_not_the_shorter_ones() {
        assert!(!starts(
            &agent_for("/Applications/AYDY.app/Contents/MacOS/aydy-next"),
            Path::new("/Applications/AYDY.app/Contents/MacOS/aydy")
        ));
    }

    /// A launch agent in the shape the autostart plugin writes them.
    fn agent_for(executable: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n  <dict>\n  \
             <key>Label</key>\n  <string>AYDY</string>\n  <key>ProgramArguments</key>\n  \
             <array><string>{executable}</string></array>\n  <key>RunAtLoad</key>\n  \
             <true/>\n  </dict>\n</plist>"
        )
    }

    /// A settings file that could not be written is the one failure where the
    /// change did happen, and saying otherwise would have the user set it
    /// twice.
    #[test]
    fn a_change_that_could_not_be_saved_says_it_is_still_in_force() {
        let directory = scratch_dir("unwritable-reason");
        let path = directory.join(SETTINGS_FILE);
        let store = SettingsStore::load(&path, Arc::new(FakeLoginItem));
        fs::remove_file(&path).expect("the settings file to go");
        fs::create_dir(&path).expect("something else to take its name");

        let failed = store
            .apply(SettingsChange {
                finished_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect_err("the write to fail");

        fs::remove_dir_all(&directory).expect("the scratch directory to be removable");

        assert_eq!(
            SettingsFailurePayload::from(&failed).reason,
            "The change is in force, but AYDY could not save it for next time."
        );
    }
}
