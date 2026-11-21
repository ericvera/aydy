# 03_05 Delete the HTTP listener and the extension registry/channel

## Goal

The app stops serving the loopback HTTP listener and stops tracking VS Code windows through extension registrations. The reconciler stops placing sessions in windows. The app reads files only.

## Files to modify/create

- src-tauri/src/lib.rs
- src-tauri/src/discovery/reconciler.rs
- src-tauri/src/discovery/window_map.rs
- src-tauri/src/session/event.rs
- src-tauri/Cargo.toml

## Files to delete

- src-tauri/src/listener/ (all of `mod.rs`, `port.rs`, `routes.rs` and `server.rs`)
- src-tauri/src/extension/ (all of `mod.rs`, `command.rs` and `registry.rs`)

## Background

- What happened in earlier tasks, so that nothing outside this task's files still imports `listener::` or `extension::`:
  - 02_01 moved `lock_recovering` to `src-tauri/src/sync.rs`; `listener/server.rs` only re-exports it.
  - 02_03 switched `notify/sound.rs` and `ipc/commands.rs` to `crate::sync` and deleted the sound focus tests.
  - 02_04 moved hook events to `discovery/file_feed.rs`.
  - 02_05 moved jump to request files, dropped every `extension::`/`listener::` import from `jump/mod.rs`, and removed the window placement (`ExtensionPid`, `Registration`) from the `ipc/commands.rs` `jumpable_session` fixture.
  - 03_03 removed health from the snapshot and the extension-status command, and switched `ipc/snapshot_channel.rs` to `crate::sync`.
  - 03_04 removed the installer, the port file and `ListenerHealth`.
- What remains:
  - `lib.rs`: the listener spawn (`listener::server::run(memory, ports, registry, extensions, ...)`), `PortMemory` and `PortState`, `PORT_MEMORY_FILE`, `ExtensionRegistry`, `ExtensionCommands` and `LoopbackChannel` if still present, `ListenerShutdown` and its trigger on exit, `Reconciler::with_windows(windows, Arc::new(SystemParents))` (`lib.rs:229-230`), and the import `use crate::discovery::process_tree::SystemParents;` (`lib.rs:23`), which becomes unused.
  - `discovery/reconciler.rs`:
    - `with_windows(Arc<ExtensionRegistry>, Arc<dyn ProcessParents>)` (`:143`) and `associate` (`:283-296`), which calls `window_map::associate(live, registration pids, parents)`.
    - Imports at `:12` (`process_tree::{ProcessParents, SystemParents}`), `:16` (`window_map`), `:17` (`ExtensionRegistry`) and `:18` (`listener::server::lock_recovering`).
    - Its test module: imports `crate::discovery::process_tree::FixedParents` (`:490`) and `crate::extension::registry::Registration` (`:496`); the helper `extension_registry()` (`:1039-1050`); and the two window tests that use it or `registry.window_of` — `a_scan_places_each_session_it_sees_in_the_window_running_it` (`.with_windows(..)` at `:1066`, `window_of` asserts at `:1072-1073`) and `a_scan_with_no_extension_involved_places_no_session_in_a_window` (`:1076-1091`).
  - `discovery/window_map.rs`: its test module (`:94-282`) imports `crate::extension::registry::{ExtensionRegistry, Registration, REGISTRATION_TIMEOUT}` (`:103`). Task 03_06 deletes the whole file.
  - `session/event.rs:5` imports `ExtensionPid`, and `:19` defines `pub type WindowId = ExtensionPid;`.
- Dependencies: `axum` is used only by the listener. `ureq` is used only by `extension/command.rs`'s `LoopbackChannel`. The tokio `net` feature may be needed only by the listener.

## Guides

None match.

## Implementation details

- `event.rs`: `pub type WindowId = u32;`, with the import dropped and the doc comment shortened. Window association is removed in 03_06 and the alias in 03_07, so keep it for now.
- `reconciler.rs`:
  - Delete `with_windows`, the `extensions` and `parents` fields and `associate`. `Scan.windows` becomes `WindowMap::default()`, or remove the field if nothing reads it. 03_06 deletes `WindowMap` entirely, so prefer the smallest edit that compiles.
  - Drop the `process_tree`, `ExtensionRegistry` and listener imports, and switch `lock_recovering` to `crate::sync`.
  - In the test module, delete the `FixedParents` and `Registration` imports, the `extension_registry()` helper and the two window tests named above. Do not rewrite them: window association is gone.
- `window_map.rs`: delete its whole `#[cfg(test)] mod tests` module. Do not rewrite it; 03_06 deletes the file. Leave the non-test code as is.
- `lib.rs`:
  - Remove `pub mod listener;` and `pub mod extension;`.
  - Remove the listener spawn and its state, `PORT_MEMORY_FILE`, `ListenerShutdown` and its exit trigger, `ExtensionRegistry`, the `with_windows` call, and the now-unused `SystemParents` import (`:23`).
- `Cargo.toml`: remove `axum` and `ureq`. Drop the tokio `net` feature only if nothing else needs it (run `cargo build` to check). `Cargo.lock` updates itself.

## Gotchas

- Grep `src-tauri/src` for `listener::`, `extension::`, `ExtensionPid`, `PortState`, `SystemParents`, `axum` and `ureq` after the change. All must be gone. If one remains in a file outside this list, an earlier task missed it: stop and report rather than widening this task.
- `process_tree.rs` keeps `SystemParents` and `FixedParents` as unused `pub` items until 03_06 deletes it. That is expected; clippy does not flag `pub` items of a library crate.
- `test_support.rs` and `discovery/mod.rs` test helpers may reference listener types. Fix any you find.
- `RunEvent` exit handling must still trigger `ScanShutdown`, `ResumeShutdown`, `PublisherShutdown` and the `FeedShutdown` from 02_04.

## Verification

- `cargo test` and `cargo clippy` from `src-tauri/`, plus `cargo build` to confirm the dependency removal.
- `yarn test`.
- Substitute check, run against the real app: launch it (`yarn tauri dev`) and confirm nothing listens on loopback for it (`lsof -iTCP -sTCP:LISTEN -P | grep -i aydy` is empty). Then confirm the file feed still drives the popover without writing over any real session's data: pick an existing file in `~/Library/Application Support/app.aydy/status/` for a live session and `touch` it (this changes only its mtime, which the feed treats as a new event at that time), and check the row reorders or turns unread in the popover. Never create, edit or overwrite a status file there. Quit the app afterwards.
