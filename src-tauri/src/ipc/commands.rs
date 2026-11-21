//! What the popover asks the app to do.
//!
//! Every command here is a thin wrapper: the registry owns unread, the tray
//! module owns the popover window, and Tauri owns exiting. Each body that has
//! anything to decide is a plain function beside its command, so it can be
//! driven without a running app.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::extension::registry::ExtensionRegistry;
use crate::hooks::install::{ChangePreview, HookInstaller};
use crate::hooks::merge::HookKind;
use crate::hooks::HookError;
use crate::jump::{JumpOutcome, JumpTarget, Jumper};
use crate::listener::port::PortState;
use crate::listener::server::{lock_recovering, ListenerHealth};
use crate::session::registry::SessionRegistry;
use crate::tray::popover::{hide, suppress_next_focus_loss, POPOVER_LABEL};

/// How long a command waits for the listener to bind before giving up.
///
/// The listener binds in the first moments of the app's life, so this only ever
/// covers a popover opened while it is still starting.
const PORT_WAIT: Duration = Duration::from_secs(2);

/// The program that reveals a file in the Finder.
const REVEAL_PROGRAM: &str = "open";

/// Whether AYDY's hooks are registered, as the first-run screen reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatusPayload {
    pub installed: bool,
    pub hook_count: usize,
    /// The settings file, written the way it is shown to the user.
    pub settings_path: String,
    /// Whether the events those hooks send are reaching the app.
    ///
    /// A second fact, not a restatement of `installed`: hooks that are
    /// registered and posting to a port nothing is listening on is exactly the
    /// state the settings pane exists to make visible.
    pub receiving_events: bool,
}

/// One hook the install will add.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedHookPayload {
    pub event: String,
    pub kind: HookKind,
    pub timeout_seconds: f64,
}

/// One entry the install will remove, named by the command it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedHookPayload {
    pub event: String,
    pub command: Option<String>,
}

/// The exact change an install will make.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPreviewPayload {
    /// How many hooks will be registered. The first-run screen states this
    /// number, and it is counted from what the installer will actually write.
    pub hook_count: usize,
    pub added: Vec<AddedHookPayload>,
    pub removed: Vec<RemovedHookPayload>,
    /// The `hooks` block exactly as it will be written.
    pub hooks_json: String,
}

impl From<&ChangePreview> for HookPreviewPayload {
    fn from(preview: &ChangePreview) -> Self {
        Self {
            hook_count: preview.hook_count(),
            added: preview
                .added
                .iter()
                .map(|hook| AddedHookPayload {
                    event: hook.event.clone(),
                    kind: hook.kind,
                    timeout_seconds: hook.timeout,
                })
                .collect(),
            removed: preview
                .removed
                .iter()
                .map(|hook| RemovedHookPayload {
                    event: hook.event.clone(),
                    command: hook.command.clone(),
                })
                .collect(),
            hooks_json: preview.hooks_json.clone(),
        }
    }
}

/// Why a hook command could not do what it was asked.
///
/// Every failure here leaves the user's settings file exactly as it was: the
/// write is atomic and aborts before it starts if the file changed, so the
/// screen can say so without qualifying it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookFailurePayload {
    pub reason: String,
}

impl From<&HookError> for HookFailurePayload {
    fn from(error: &HookError) -> Self {
        let reason = match error {
            HookError::NoHome => "AYDY could not find your home directory.",
            HookError::Read { .. } => "The file could not be read.",
            HookError::Write { .. } => "The file could not be written.",
            HookError::Malformed { .. } => "The file is not valid JSON, so AYDY left it alone.",
            HookError::Unexpected { .. } => {
                "The file is not in the shape AYDY expects, so it left it alone."
            }
            HookError::ChangedOnDisk { .. } => "The file changed while AYDY was writing it.",
        };

        Self {
            reason: reason.to_string(),
        }
    }
}

/// A path written the way the user wrote it, with their home directory back as
/// `~`.
///
/// The first-run screen names the file it is asking to edit, and
/// `~/.claude/settings.json` is how that file is known — an absolute path with
/// a user name in it is both longer than the popover and less recognisable.
pub fn home_relative(path: &Path, home: Option<&Path>) -> String {
    let display = path.display().to_string();

    let Some(home) = home else {
        return display;
    };

    match path.strip_prefix(home) {
        Ok(rest) => Path::new("~").join(rest).display().to_string(),
        Err(_) => display,
    }
}

/// The home directory, as every other part of the app reads it.
fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// The program and arguments that reveal `path` in the Finder.
///
/// `open -R` selects the file in a Finder window rather than opening it, which
/// is what **Reveal file** offers: the user is being sent to look at their
/// settings file, not to have it opened by whatever owns `.json`.
pub fn reveal_command(path: &Path) -> (&'static str, Vec<&OsStr>) {
    (REVEAL_PROGRAM, vec![OsStr::new("-R"), path.as_os_str()])
}

/// Marks a session read, which is what clicking its row does.
///
/// This is the only way unread clears without the companion extension, so it
/// runs for every row — a row that cannot be jumped to still clears.
#[tauri::command]
pub fn mark_read(session_id: String, registry: State<'_, Arc<Mutex<SessionRegistry>>>) {
    mark_session_read(&registry, &session_id);
}

/// The body of [`mark_read`], reachable without a Tauri app to hold state.
///
/// A session the registry no longer holds is not an error: the row was
/// rendered from a snapshot that can be a fraction of a second old, and the
/// session it named may have ended in between.
pub fn mark_session_read(registry: &Mutex<SessionRegistry>, session_id: &str) {
    lock_recovering(registry).mark_read(session_id);
}

/// Takes the user to a session, which is what clicking a jumpable row does.
///
/// It answers what happened rather than acknowledging the click. Every
/// mechanism a jump is made of fails silently, so the popover is told whether
/// the window came forward, whether nothing could be found to raise — which is
/// a click that goes nowhere and says nothing, not an error — and whether the
/// reveal opened a tab it should not have.
#[tauri::command]
pub async fn jump_to_session(
    session_id: String,
    registry: State<'_, Arc<Mutex<SessionRegistry>>>,
    jumper: State<'_, Arc<Jumper>>,
) -> Result<JumpOutcome, String> {
    let Some(target) = jump_target(&registry, &session_id) else {
        return Ok(JumpOutcome::NoWindow);
    };

    let jumper = Arc::clone(&jumper);

    // A jump posts to a window, runs a command line and then waits for a
    // report to arrive, so it runs on a thread that is allowed to wait:
    // blocking an async worker can starve the runtime's IO driver, which is
    // what the listener — and therefore the confirmation this is waiting for —
    // answers on.
    Ok(
        tauri::async_runtime::spawn_blocking(move || jumper.jump(&target))
            .await
            // A jump that could not even be run is a jump nothing confirmed.
            .unwrap_or(JumpOutcome::Unconfirmed),
    )
}

/// What jumping to `session_id` would have to do, reachable without a Tauri app
/// to hold state.
///
/// A session the registry no longer holds, and one that was never jumpable,
/// both answer `None`: the row was rendered from a snapshot that can be a
/// fraction of a second old.
pub fn jump_target(registry: &Mutex<SessionRegistry>, session_id: &str) -> Option<JumpTarget> {
    JumpTarget::of(lock_recovering(registry).session(session_id)?)
}

/// Takes the popover down.
///
/// A click that navigates closes the popover; a click that only marks a
/// session read leaves it open. The hide goes through the tray module rather
/// than the window API so it is recorded the same way every other hide is, and
/// the tray icon's click-to-close cannot be thrown by it.
#[tauri::command]
pub fn close_popover(app: AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        hide(&window);
    }
}

/// Quits the app.
///
/// Exiting through the app handle is what raises the run events the listener,
/// the scan, the resume watch and the publisher stop on.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// Whether AYDY's hooks are registered.
///
/// Read from the settings file rather than from memory, because the file is the
/// user's and they may have taken the hooks out of it since the app started.
#[tauri::command]
pub async fn hook_status(
    hooks: State<'_, Arc<HookInstaller>>,
    health: State<'_, Arc<ListenerHealth>>,
) -> Result<HookStatusPayload, HookFailurePayload> {
    let installer = Arc::clone(&hooks);
    let health = Arc::clone(&health);

    tauri::async_runtime::spawn_blocking(move || status_payload(&installer, &health))
        .await
        .map_err(|_| interrupted())
}

/// The exact change an install would make, for the screen that asks the user to
/// confirm it.
#[tauri::command]
pub async fn preview_hook_install(
    hooks: State<'_, Arc<HookInstaller>>,
    ports: State<'_, Arc<PortState>>,
) -> Result<HookPreviewPayload, HookFailurePayload> {
    let installer = Arc::clone(&hooks);
    let port = listening_port(&Arc::clone(&ports)).await?;

    tauri::async_runtime::spawn_blocking(move || {
        installer
            .preview_install(port)
            .map(|preview| HookPreviewPayload::from(&preview))
    })
    .await
    .map_err(|_| interrupted())?
    .map_err(|error| HookFailurePayload::from(&error))
}

/// Registers AYDY's hooks, which the user has just confirmed.
#[tauri::command]
pub async fn install_hooks(
    hooks: State<'_, Arc<HookInstaller>>,
    ports: State<'_, Arc<PortState>>,
) -> Result<(), HookFailurePayload> {
    let installer = Arc::clone(&hooks);
    let port = listening_port(&Arc::clone(&ports)).await?;

    tauri::async_runtime::spawn_blocking(move || installer.install(port).map(|_| ()))
        .await
        .map_err(|_| interrupted())?
        .map_err(|error| HookFailurePayload::from(&error))
}

/// Repairs AYDY's hook entries without an uninstall first.
///
/// The port is read again rather than assumed, so a reinstall is also how
/// entries left pointing at a port the listener no longer holds are put right.
#[tauri::command]
pub async fn reinstall_hooks(
    hooks: State<'_, Arc<HookInstaller>>,
    ports: State<'_, Arc<PortState>>,
) -> Result<(), HookFailurePayload> {
    let installer = Arc::clone(&hooks);
    let port = listening_port(&Arc::clone(&ports)).await?;

    tauri::async_runtime::spawn_blocking(move || installer.reinstall(port).map(|_| ()))
        .await
        .map_err(|_| interrupted())?
        .map_err(|error| HookFailurePayload::from(&error))
}

/// Removes AYDY's hook entries and puts back the ones it took over.
///
/// The app keeps running: it goes back to asking for its hooks, which is the
/// state it is in before they are installed.
#[tauri::command]
pub async fn uninstall_hooks(
    hooks: State<'_, Arc<HookInstaller>>,
) -> Result<(), HookFailurePayload> {
    let installer = Arc::clone(&hooks);

    tauri::async_runtime::spawn_blocking(move || installer.uninstall().map(|_| ()))
        .await
        .map_err(|_| interrupted())?
        .map_err(|error| HookFailurePayload::from(&error))
}

/// Shows the user their settings file in the Finder.
///
/// This is the other half of a failed write: the app says it changed nothing,
/// and this is how the user goes and looks.
#[tauri::command]
pub async fn reveal_settings_file(
    hooks: State<'_, Arc<HookInstaller>>,
) -> Result<(), HookFailurePayload> {
    let installer = Arc::clone(&hooks);

    tauri::async_runtime::spawn_blocking(move || reveal(installer.settings_path()))
        .await
        .map_err(|_| interrupted())?
}

/// Whether the companion extension has registered, as the settings pane reads
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatusPayload {
    pub connected: bool,
    /// How many VS Code windows are registered. One extension host per window,
    /// so this counts windows rather than installations.
    pub window_count: usize,
}

/// Whether any VS Code window has registered with AYDY.
///
/// Read when the pane opens rather than remembered, because a window closing
/// says nothing: its registration goes quiet and is dropped, and only asking
/// again finds that out.
#[tauri::command]
pub fn extension_status(windows: State<'_, Arc<ExtensionRegistry>>) -> ExtensionStatusPayload {
    connection_payload(&windows)
}

/// The body of [`extension_status`], reachable without a Tauri app to hold
/// state.
pub fn connection_payload(windows: &ExtensionRegistry) -> ExtensionStatusPayload {
    ExtensionStatusPayload {
        window_count: windows.window_count(),
        connected: windows.connected(),
    }
}

/// The body of [`hook_status`], reachable without a Tauri app to hold state.
///
/// The file is read first and the health asked afterwards, in that order:
/// reading is what tells the listener's health which hooks are registered and
/// which port they name.
pub fn status_payload(installer: &HookInstaller, health: &ListenerHealth) -> HookStatusPayload {
    let status = installer.status();

    HookStatusPayload {
        installed: status.installed,
        hook_count: status.hook_count,
        settings_path: home_relative(installer.settings_path(), user_home().as_deref()),
        // Hooks that do not exist send nothing, so nothing is arriving. The
        // health answers false for that case because a fresh install is not
        // broken; the pane needs it as the fact it is, which is why the two are
        // combined here rather than in the interface.
        receiving_events: status.installed && !health.cannot_receive_events(),
    }
}

/// Opens a Finder window with `path` selected in it.
fn reveal(path: &Path) -> Result<(), HookFailurePayload> {
    match revealing(path).status() {
        Ok(status) if status.success() => Ok(()),
        _ => Err(HookFailurePayload {
            reason: "The Finder could not be opened.".to_string(),
        }),
    }
}

/// The Finder command for `path`, with the popover's next focus loss armed.
///
/// The arming sits here, on the line before the command runs: the Finder comes
/// forward within milliseconds, and the focus loss that follows would otherwise
/// take the popover down underneath the thing it just opened.
fn revealing(path: &Path) -> Command {
    suppress_next_focus_loss();

    let (program, arguments) = reveal_command(path);
    let mut command = Command::new(program);
    command.args(arguments);

    command
}

/// The port the listener is answering on.
///
/// The hooks being written have to name it, so a command that needs it waits
/// briefly rather than failing on a popover opened while the app is still
/// starting.
async fn listening_port(ports: &PortState) -> Result<u16, HookFailurePayload> {
    if let Some(port) = ports.port() {
        return Ok(port);
    }

    match tokio::time::timeout(PORT_WAIT, ports.wait_for_port()).await {
        Ok(Some(port)) => Ok(port),
        _ => Err(HookFailurePayload {
            reason: "AYDY is not listening for hook events yet.".to_string(),
        }),
    }
}

/// A task that did not finish, which is not something the user can act on.
fn interrupted() -> HookFailurePayload {
    HookFailurePayload {
        reason: "AYDY could not finish reading the file.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Instant;

    use tauri::WindowEvent;

    use super::*;
    use crate::clock::TestClock;
    use crate::discovery::process_tree::FixedParents;
    use crate::discovery::session_files::SessionFile;
    use crate::discovery::window_map::associate;
    use crate::extension::registry::{ExtensionPid, Registration};
    use crate::hooks::install::{AddedHook, HookRegistry};
    use crate::hooks::settings_file::SettingsFile;
    use crate::hooks::stash::HookStash;
    use crate::hooks::test_support::{scratch_dir, REAL_SETTINGS, SETTINGS_WITHOUT_HOOKS};
    use crate::session::event::{HookEvent, HookEventKind, SessionEnrichment};
    use crate::tray::popover::hides_the_popover;

    /// The port an install writes into the hook URLs in these tests.
    const PORT: u16 = 51_234;

    /// An installer over a scratch copy of a settings file. No test here ever
    /// reads or writes the developer's own `~/.claude/settings.json`.
    fn scratch_installer(name: &str, contents: &str) -> HookInstaller {
        let directory = scratch_dir(name);
        let settings = directory.join("settings.json");
        fs::write(&settings, contents).expect("the fixture to be written");

        HookInstaller::new(
            SettingsFile::new(&settings),
            HookStash::new(directory.join("removed-hooks.json")),
            Arc::new(HookRegistry::new()),
        )
    }

    /// The health the settings pane reads, over the installer's own registry —
    /// which is the registry `status()` fills in, so the two facts come from
    /// one reading of the file.
    fn health_bound_to(installer: &HookInstaller, port: Option<u16>) -> ListenerHealth {
        let ports = Arc::new(PortState::new());

        if let Some(port) = port {
            ports.set_bound(port, None);
        }

        ListenerHealth::new(ports, installer.registry())
    }

    #[test]
    fn a_settings_file_that_holds_no_aydy_hooks_reports_none_installed() {
        let installer = scratch_installer("status-absent", SETTINGS_WITHOUT_HOOKS);
        let health = health_bound_to(&installer, Some(PORT));

        let status = status_payload(&installer, &health);

        assert!(!status.installed);
        assert_eq!(status.hook_count, 0);
    }

    /// The first-run screen states this number, so it has to be the number the
    /// installer actually registered rather than a constant either side of the
    /// boundary agreed on.
    #[test]
    fn the_reported_hook_count_is_the_number_of_entries_in_the_file() {
        let installer = scratch_installer("status-installed", REAL_SETTINGS);
        let registered = installer
            .install(PORT)
            .expect("the install to run")
            .hooks_registered;
        let health = health_bound_to(&installer, Some(PORT));

        let status = status_payload(&installer, &health);

        assert!(status.installed);
        assert_eq!(status.hook_count, registered);
    }

    #[test]
    fn the_settings_path_is_shown_with_the_home_directory_as_a_tilde() {
        let home = Path::new("/Users/someone");

        assert_eq!(
            home_relative(&home.join(".claude/settings.json"), Some(home)),
            "~/.claude/settings.json"
        );
    }

    /// `CLAUDE_CONFIG_DIR` can put the file anywhere, including outside the
    /// home directory, and a path that is not under it is shown as it is.
    #[test]
    fn a_settings_path_outside_the_home_directory_is_shown_in_full() {
        assert_eq!(
            home_relative(
                Path::new("/etc/claude/settings.json"),
                Some(Path::new("/Users/someone"))
            ),
            "/etc/claude/settings.json"
        );
        assert_eq!(
            home_relative(Path::new("/etc/claude/settings.json"), None),
            "/etc/claude/settings.json"
        );
    }

    /// The preview is what the first-run screen renders, so both halves of the
    /// change have to survive the crossing: what is added and what is removed.
    #[test]
    fn the_preview_carries_the_hooks_to_be_added_and_the_entries_to_be_removed() {
        let installer = scratch_installer("preview", REAL_SETTINGS);
        let preview = installer.preview_install(PORT).expect("a preview");

        let payload = HookPreviewPayload::from(&preview);

        assert_eq!(payload.hook_count, preview.added.len());
        assert_eq!(payload.added.len(), preview.added.len());
        assert_eq!(payload.removed.len(), preview.removed.len());
        assert_eq!(payload.hooks_json, preview.hooks_json);

        let added = payload.added.first().expect("a hook to be added");
        let first = preview.added.first().expect("a hook to be added");
        assert_eq!(added.event, first.event);
        assert_eq!(added.kind, first.kind);
        assert_eq!(added.timeout_seconds, first.timeout);

        let removed = payload.removed.first().expect("an entry to be removed");
        let first = preview.removed.first().expect("an entry to be removed");
        assert_eq!(removed.event, first.event);
        assert_eq!(removed.command, first.command);
        assert!(
            removed.command.is_some(),
            "an entry the user is asked about has to be named by something"
        );
    }

    /// The count the first-run screen states is counted from what will be
    /// written, so a preview carrying any other number of hooks reports that
    /// number. A five anywhere in this chain is the thing that would go on
    /// being right until the day a sixth hook was registered.
    #[test]
    fn the_hook_count_follows_the_preview_rather_than_a_number_of_its_own() {
        let preview = ChangePreview {
            settings_path: "/tmp/settings.json".to_string(),
            added: vec![
                AddedHook {
                    event: "Stop".to_string(),
                    kind: HookKind::Http,
                    timeout: 2.0,
                },
                AddedHook {
                    event: "SessionEnd".to_string(),
                    kind: HookKind::Http,
                    timeout: 0.5,
                },
            ],
            removed: Vec::new(),
            hooks_json: "{}".to_string(),
        };

        assert_eq!(HookPreviewPayload::from(&preview).hook_count, 2);
    }

    /// A settings file the app cannot even read is not a reason to hide the
    /// screen that asks to write it: the first-run screen is what offers the
    /// retry and the way to go and look at the file.
    #[test]
    fn a_settings_file_that_cannot_be_read_reports_nothing_installed() {
        let installer = scratch_installer("status-unreadable", "{ not json");
        let health = health_bound_to(&installer, Some(PORT));

        let status = status_payload(&installer, &health);

        assert!(!status.installed);
        assert_eq!(status.hook_count, 0);
    }

    /// The two facts the settings pane keeps apart. Hooks registered against a
    /// port nothing is listening on are installed and silent, and collapsing
    /// that into one answer hides the only state the port-move notice exists to
    /// explain.
    #[test]
    fn whether_events_are_arriving_is_a_fact_of_its_own() {
        let installer = scratch_installer("receiving", REAL_SETTINGS);
        installer.install(PORT).expect("the install to run");

        let listening = status_payload(&installer, &health_bound_to(&installer, Some(PORT)));
        assert!(listening.installed);
        assert!(listening.receiving_events);

        let elsewhere = status_payload(&installer, &health_bound_to(&installer, Some(PORT + 1)));
        assert!(elsewhere.installed);
        assert!(!elsewhere.receiving_events);

        let silent = status_payload(&installer, &health_bound_to(&installer, None));
        assert!(silent.installed);
        assert!(!silent.receiving_events);
    }

    /// Nothing is being sent before the hooks exist, so nothing is arriving.
    /// The health answers false for that case — a fresh install is not broken —
    /// and the pane needs the fact rather than the reassurance.
    #[test]
    fn hooks_that_do_not_exist_are_not_receiving_events() {
        let installer = scratch_installer("receiving-absent", SETTINGS_WITHOUT_HOOKS);
        let health = health_bound_to(&installer, Some(PORT));

        let status = status_payload(&installer, &health);

        assert!(!status.installed);
        assert!(!status.receiving_events);
        assert!(!health.cannot_receive_events());
    }

    /// Removing the hooks leaves the app running and the file as it was found,
    /// which is what makes the pane's remove action safe to offer: it puts the
    /// user back at the first-run screen rather than ending the app.
    #[test]
    fn removing_the_hooks_puts_the_file_back_and_reports_nothing_installed() {
        let installer = scratch_installer("uninstall-status", REAL_SETTINGS);
        installer.install(PORT).expect("the install to run");

        installer.uninstall().expect("the uninstall to run");

        let status = status_payload(&installer, &health_bound_to(&installer, Some(PORT)));
        assert!(!status.installed);
        assert_eq!(status.hook_count, 0);
        assert!(!status.receiving_events);
    }

    /// A reinstall repairs the entries in place, which is what points hooks
    /// left on an old port back at the one the listener holds.
    #[test]
    fn reinstalling_points_the_hooks_at_the_port_the_listener_holds() {
        let installer = scratch_installer("reinstall-status", REAL_SETTINGS);
        installer.install(PORT).expect("the install to run");
        let moved = PORT + 7;

        installer.reinstall(moved).expect("the reinstall to run");

        let status = status_payload(&installer, &health_bound_to(&installer, Some(moved)));
        assert!(status.installed);
        assert!(status.receiving_events);
    }

    /// The failure the user needs to know their file is intact after.
    #[test]
    fn a_file_that_changed_under_the_write_is_reported_as_such() {
        let failure = HookFailurePayload::from(&HookError::ChangedOnDisk {
            path: PathBuf::from("/tmp/settings.json"),
        });

        assert_eq!(
            failure.reason,
            "The file changed while AYDY was writing it."
        );
    }

    #[test]
    fn every_failure_names_a_reason_of_its_own() {
        let path = PathBuf::from("/tmp/settings.json");
        let reasons: Vec<String> = [
            HookError::NoHome,
            HookError::Read {
                path: path.clone(),
                source: std::io::Error::other("no"),
            },
            HookError::Write {
                path: path.clone(),
                source: std::io::Error::other("no"),
            },
            HookError::Malformed {
                path: path.clone(),
                source: serde_json::from_str::<serde_json::Value>("{").expect_err("a parse error"),
            },
            HookError::Unexpected {
                path: path.clone(),
                at: "hooks".to_string(),
            },
            HookError::ChangedOnDisk { path },
        ]
        .iter()
        .map(|error| HookFailurePayload::from(error).reason)
        .collect();

        let mut distinct = reasons.clone();
        distinct.sort();
        distinct.dedup();

        assert_eq!(distinct.len(), reasons.len(), "got {reasons:?}");
        assert!(reasons.iter().all(|reason| reason.ends_with('.')));
    }

    /// **Reveal file** has to send the user to the file, not open it in
    /// whatever owns `.json`.
    #[test]
    fn revealing_the_settings_file_selects_it_in_the_finder() {
        let path = Path::new("/Users/someone/.claude/settings.json");

        let (program, arguments) = reveal_command(path);

        assert_eq!(program, "open");
        assert_eq!(
            arguments,
            vec![OsStr::new("-R"), OsStr::new(path.as_os_str())]
        );
    }

    /// The two ends of the focus suppression, which is one line at each end and
    /// covered by neither the tests of `FocusSuppression` nor those of the
    /// commands: **Reveal file** arms it, and the popover's window event handler
    /// is what consumes it. Without the arming the Finder's focus loss takes the
    /// popover down underneath the window it just opened; without the consuming
    /// the popover could never be dismissed by clicking away.
    #[test]
    fn revealing_the_settings_file_leaves_the_popover_up_while_the_finder_takes_focus() {
        let now = Instant::now();

        let command = revealing(Path::new("/Users/someone/.claude/settings.json"));

        assert_eq!(command.get_program(), OsStr::new(REVEAL_PROGRAM));
        assert!(
            !hides_the_popover(&WindowEvent::Focused(false), now),
            "the focus loss the Finder causes is the one Reveal file armed"
        );
        assert!(
            hides_the_popover(&WindowEvent::Focused(false), now),
            "every other focus loss still dismisses the popover"
        );
    }

    /// A registry holding one session that a `Stop` has left unread.
    fn unread_session() -> Mutex<SessionRegistry> {
        let mut registry = SessionRegistry::new(Arc::new(TestClock::new()));

        registry.apply_hook_event(HookEvent::new("session-1", HookEventKind::UserPromptSubmit));
        registry.apply_hook_event(HookEvent::new("session-1", HookEventKind::Stop));

        Mutex::new(registry)
    }

    /// The extension host of the window the session below is placed in.
    const WINDOW: ExtensionPid = 65_921;

    /// A session the scan found in VS Code, the transcript pass titled, and
    /// the process-tree join placed in a window — built through the same three
    /// entry points the running app builds it through.
    fn jumpable_session() -> Mutex<SessionRegistry> {
        let mut registry = SessionRegistry::new(Arc::new(TestClock::new()));
        let claude = 90_001;

        registry.apply_hook_event(
            HookEvent::new("session-1", HookEventKind::UserPromptSubmit)
                .with_cwd("/Users/eric/Code/aydy"),
        );
        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                label: Some("Models section configuration options".to_string()),
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );
        registry.apply_window_map(&associate(
            &[SessionFile {
                pid: claude,
                session_id: "session-1".to_string(),
                cwd: None,
                entrypoint: None,
            }],
            &[WINDOW],
            &FixedParents::new(vec![(claude, WINDOW)]),
        ));

        Mutex::new(registry)
    }

    fn is_unread(registry: &Mutex<SessionRegistry>, session_id: &str) -> bool {
        lock_recovering(registry)
            .session(session_id)
            .expect("the session to be tracked")
            .unread
    }

    /// A session placed in a window by the scan, labelled by the transcript
    /// pass, and read back out the way the command reads it.
    #[test]
    fn a_jumpable_session_carries_its_window_its_tab_and_its_directory() {
        let registry = jumpable_session();

        let target = jump_target(&registry, "session-1").expect("a target");

        assert_eq!(target.session_id, "session-1");
        assert_eq!(target.window, Some(WINDOW));
        assert_eq!(
            target.tab_label,
            Some("Models section configura\u{2026}".to_string())
        );
        assert_eq!(target.cwd, Some(PathBuf::from("/Users/eric/Code/aydy")));
    }

    /// The click still marks the row read; there is simply nowhere to send it.
    #[test]
    fn a_session_that_cannot_be_jumped_to_has_no_target() {
        let registry = unread_session();

        assert_eq!(jump_target(&registry, "session-1"), None);
    }

    /// The row was rendered from a snapshot that can be a fraction of a second
    /// old, and the session it named may have ended in between.
    #[test]
    fn a_session_the_registry_no_longer_holds_has_no_target() {
        let registry = unread_session();

        assert_eq!(jump_target(&registry, "session-gone"), None);
    }

    #[test]
    fn clicking_a_row_marks_its_session_read() {
        let registry = unread_session();
        assert!(is_unread(&registry, "session-1"));

        mark_session_read(&registry, "session-1");

        assert!(!is_unread(&registry, "session-1"));
    }

    #[test]
    fn clicking_one_row_leaves_every_other_session_unread() {
        let registry = unread_session();
        lock_recovering(&registry)
            .apply_hook_event(HookEvent::new("session-2", HookEventKind::UserPromptSubmit));
        lock_recovering(&registry)
            .apply_hook_event(HookEvent::new("session-2", HookEventKind::Stop));

        mark_session_read(&registry, "session-1");

        assert!(!is_unread(&registry, "session-1"));
        assert!(is_unread(&registry, "session-2"));
    }

    /// The snapshot a row was rendered from can be a fraction of a second old,
    /// so the session it names may already have ended. That click must do
    /// nothing rather than panic, because a panic here takes the command out.
    #[test]
    fn clicking_a_row_for_a_session_that_has_gone_does_nothing() {
        let registry = unread_session();

        mark_session_read(&registry, "session-that-ended");

        assert!(is_unread(&registry, "session-1"));
    }

    /// A poisoned registry lock is recovered from rather than propagated: a
    /// panic somewhere else must not leave clicking a row dead for the rest of
    /// the session.
    #[test]
    fn a_click_still_lands_after_a_panic_poisoned_the_registry() {
        let registry = Arc::new(unread_session());
        let poisoning = Arc::clone(&registry);

        let panicked = std::thread::spawn(move || {
            let _guard = poisoning.lock().expect("the lock");

            panic!("something near the registry gave up");
        })
        .join();

        assert!(panicked.is_err());
        assert!(registry.is_poisoned());

        mark_session_read(&registry, "session-1");

        assert!(!is_unread(&registry, "session-1"));
    }

    /// Working time keeps accruing while a session is Working, so the reading
    /// the popover shows is a live one rather than a value frozen at the last
    /// hook event.
    #[test]
    fn a_session_marked_read_keeps_the_rest_of_its_state() {
        let clock = Arc::new(TestClock::new());
        let mut registry = SessionRegistry::new(clock.clone());

        registry.apply_hook_event(HookEvent::new("session-1", HookEventKind::UserPromptSubmit));
        clock.advance(Duration::from_secs(60));

        let registry = Mutex::new(registry);
        mark_session_read(&registry, "session-1");

        let snapshot = lock_recovering(&registry).snapshot();
        let session = snapshot.groups[0].sessions[0].clone();

        assert!(!session.unread);
        assert_eq!(session.working_time, Duration::from_secs(60));
    }

    fn window(pid: u32) -> Registration {
        Registration {
            pid,
            callback_port: 55_123,
            workspace_folders: vec![PathBuf::from("/Users/eric/Code/aydy")],
            claude_code_version: Some("2.1.238".to_string()),
        }
    }

    #[test]
    fn the_extension_reads_as_not_connected_until_a_window_registers() {
        let windows = ExtensionRegistry::new(Arc::new(TestClock::new()));

        let status = connection_payload(&windows);

        assert!(!status.connected);
        assert_eq!(status.window_count, 0);
    }

    #[test]
    fn the_extension_reads_as_connected_once_a_window_has_registered() {
        let windows = ExtensionRegistry::new(Arc::new(TestClock::new()));
        windows.register(window(65_921));
        windows.register(window(17_804));

        let status = connection_payload(&windows);

        assert!(status.connected);
        assert_eq!(status.window_count, 2);
    }

    /// The pane is opened long after the windows were closed, and nothing
    /// announced their going. Reading is what finds that out.
    #[test]
    fn the_extension_reads_as_not_connected_again_once_every_window_has_gone() {
        let windows = ExtensionRegistry::new(Arc::new(TestClock::new()));
        windows.register(window(65_921));

        windows.forget(65_921);

        assert!(!connection_payload(&windows).connected);
    }
}
