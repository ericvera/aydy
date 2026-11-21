# 03_03 Drop listener health, hook and extension IPC, and the not-receiving icon

## Goal

The snapshot sent to the popover stops carrying listener health, port moves, the stale list, the extension connection and the current session. The hook-installer and extension-status IPC commands go, and the menu bar icon loses its not-receiving state.

## Files to modify/create

- src-tauri/src/ipc/snapshot_channel.rs
- src-tauri/src/ipc/commands.rs
- src-tauri/src/tray/icon.rs
- src-tauri/src/lib.rs

## Files to delete

- src-tauri/icons/tray/not-receiving.png (an asset, along with any `@2x` sibling)

## Background

- The frontend stopped reading `portMove`, `stale`, `extensionConnected` and `isCurrent` in tasks 03_01 and 03_02. It no longer calls `hook_status`, `preview_hook_install`, `install_hooks`, `reinstall_hooks`, `uninstall_hooks`, `reveal_settings_file` or `extension_status`.
- `ipc/snapshot_channel.rs`:
  - `run(app, registry, health: Arc<ListenerHealth>, ports: Arc<PortState>, windows: Arc<ExtensionRegistry>, subscription, sounds, shutdown)` (`:382`).
  - It reads `health.cannot_receive_events()` (`:434`), `windows.connected()` (`:437`) and `ports.moved()` (`:449`).
  - Payload pieces: `SnapshotPayload` (`:215`, with `port_move`, `stale` and `extension_connected`), `PortMovePayload` (`:146`), `StalePayload` (`:166`) and `Staleness` (`:179-205`).
  - `SessionPayload.is_current` (`:95`).
  - `observe()` (`:278-300`) calls `indication(snapshot, cannot_receive_events)`.
- `tray/icon.rs`: `IconState::NotReceiving` (`:17`, its image at `:33`), `indication` (`:56-61`) and the test table at `:368`.
- `ipc/commands.rs`:
  - The hook commands and helpers are at `:260-357` and `:391+`: `HookStatusPayload` (`:39`), `HookFailurePayload`, `home_relative` and `reveal`.
  - `extension_status` and `ExtensionStatusPayload` are at `:360-380`.
- `lib.rs`: `generate_handler!` (`:101-116`), the snapshot spawn (`:203-212`) and the `ListenerHealth` creation and manage (`:132`, `:174`).
- `listener/server.rs`: `ListenerHealth` (`:278-300`) stays defined in this task. The tests of `hooks/install.rs` (`:511`, `:1077-1112`) construct it, and that file is deleted in 03_04, which deletes `ListenerHealth` with it. `HookRegistration` and `RegisteredHooks` (`:261-276`) stay too, because `hooks/install.rs:153` still implements them.
- The `ipc/commands.rs` test module imports installer types used only by the hook command tests: `crate::hooks::install::{AddedHook, HookRegistry}` (`:469`), `crate::hooks::stash::HookStash` (`:471`), `crate::hooks::settings_file::SettingsFile` and `crate::hooks::test_support::{scratch_dir, REAL_SETTINGS, SETTINGS_WITHOUT_HOOKS}`, with helpers such as `const PORT` and the scratch installer and `health_bound_to` (`:496-503`). `hooks/install.rs` and `hooks/stash.rs` are deleted in 03_04.
- `lock_recovering` now lives in `src-tauri/src/sync.rs` (task 02_01).

## Guides

None match.

## Implementation details

- `snapshot_channel.rs`:
  - Drop the `health`, `ports` and `windows` params.
  - Delete `PortMovePayload`, `StalePayload`, `Staleness` and the three payload fields.
  - Delete `SessionPayload.is_current`. The registry's `SessionSnapshot.is_current` stays until 03_07, so just stop mapping it, and keep `is_current` in the test fixtures that build `SessionSnapshot` literals (`:565`, `:874`, `:910`).
  - `indication(snapshot)` loses its flag.
  - Import `lock_recovering` from `crate::sync`.
- `tray/icon.rs`: remove `NotReceiving` and its asset include, simplify `indication`, and update the tests and doc comments.
- `commands.rs`: delete the seven commands and their payloads and helpers, and the imports of `hooks::install`, `hooks::merge::HookKind`, `ListenerHealth`, `PortState` and `ExtensionRegistry`. In the test module, delete the tests for those commands together with every helper, constant and import only they used (the installer and stash imports above, `PORT`, the scratch installer, `health_bound_to`). Keep whatever the remaining tests (mark read, jump, popover) use.
- `lib.rs`:
  - Remove those seven from `generate_handler!`.
  - Stop creating and managing `ListenerHealth`.
  - Pass the new `run` arguments.
  - If nothing in `lib.rs` uses `ListenerHealth` any more, drop its import. The type itself stays in `listener/server.rs` until 03_04.
  - The listener, `PortState`, `HookInstaller` and `follow_port_moves` remain for now (03_04 and 03_05).

## Gotchas

- `HookInstaller` may still be managed, because `follow_port_moves` uses it. Remove only what this task names.
- Check `src-tauri/icons/tray/` for every not-receiving variant before deleting, and make sure nothing else includes them.

## Verification

- Update the tests:
  - `snapshot_channel.rs` payload tests assert the camelCase payload has no `portMove`, `stale`, `extensionConnected` or `isCurrent`.
  - `icon.rs` indication tests.
  - Remove the `commands.rs` tests for the deleted commands. Grep the `commands.rs` test module for `hooks::install`, `hooks::stash`, `HookInstaller` and `ListenerHealth`; none may remain.
- Run `cargo test` and `cargo clippy` from `src-tauri/`, and `yarn test`.
