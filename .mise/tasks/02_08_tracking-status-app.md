# 02_08 `tracking_status` IPC command

## Goal

The app can tell whether the extension's session tracking is installed. It reports one of three states: installed, not installed (no hook script), or hooks missing (script present but some hook entry absent). The popover uses this to show the "Install the extension" screen.

## Files to modify/create

- src-tauri/src/tracking.rs (new)
- src-tauri/src/lib.rs
- src-tauri/src/ipc/commands.rs

## Background

- The extension (stage 1) installs:
  - the script at `<data>/bin/aydy-hook.sh` (`DataDir::hook_script()` in `src-tauri/src/discovery/status_files.rs`, task 02_02)
  - one command hook per event for `SessionStart`, `UserPromptSubmit`, `Notification`, `Stop` and `SessionEnd` in the Claude settings file, each with a `command` containing the marker `app.aydy/bin/aydy-hook.sh`
- Reading the settings file: `src-tauri/src/hooks/settings_file.rs`.
  - `SettingsFile::user()` (`:131`) honours `CLAUDE_CONFIG_DIR`.
  - `read()` (`:144`) returns a `SettingsSnapshot` with `value()`.
  - Errors are `HookError` (`src-tauri/src/hooks/mod.rs:25`).

  This module survives stage 3. Only the installer is removed.
- The command and payload pattern: `ipc/commands.rs` (for example `extension_status` at `:360-380`, with a serde camelCase payload). Commands are registered in `lib.rs:101-116`.
- States from the mock (`.mise/mock/popover-install-extension.html`):
  - A or B: the script is missing.
  - C: the script is present and the hook entries are gone.

## Guides

None match.

## Implementation details

- `tracking.rs`:
  - `pub const HOOK_SCRIPT_MARKER: &str = "app.aydy/bin/aydy-hook.sh"` and `pub const TRACKED_EVENTS: [&str; 5]`.
  - `#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub enum TrackingState { Installed, NotInstalled, HooksMissing }`.
  - `pub fn hooks_present(settings: &serde_json::Value) -> bool` is true when every event has at least one hook object whose `type == "command"` and whose `command` contains the marker.
  - `pub fn tracking_state(script: &Path, settings: &SettingsFile) -> TrackingState`:
    - No script file gives `NotInstalled`.
    - A settings read error, or hooks not present, gives `HooksMissing`.
    - Otherwise `Installed`.
- `commands.rs`: `#[tauri::command] pub fn tracking_status(data: State<'_, DataDir>) -> TrackingStatusPayload { state }`. It uses `SettingsFile::user()`, and on `Err` returns `HooksMissing` when the script exists, `NotInstalled` otherwise.
- `lib.rs`: add `pub mod tracking;` and register `tracking_status` in `generate_handler!`.

## Gotchas

- Only `hooks_present` decides "installed". Do not use `hooks::merge::is_aydy_hook`. That function identifies the legacy `X-Aydy-Hook` entries, which are a different thing.
- The command runs often (the popover polls every 2 s while not installed). Keep it to one stat and one file read.

## Verification

- Unit tests in `tracking.rs` using scratch dirs (`crate::test_support::scratch_dir`):
  - no script gives NotInstalled
  - script with no settings file gives HooksMissing
  - script with only 4 of 5 events gives HooksMissing
  - script with all 5 gives Installed
  - unrelated hooks and legacy `X-Aydy-Hook` entries alone give HooksMissing
  - invalid JSON gives HooksMissing
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
