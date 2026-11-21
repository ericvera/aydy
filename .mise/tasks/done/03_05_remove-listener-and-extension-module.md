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
    - Imports at `:12` (`process_tree::{ProcessParents, SystemParents}`), `:16` (`window_map::{self, WindowMap}`), `:17` (`ExtensionRegistry`), `:18` (`listener::server::lock_recovering`) and the `WindowId` in the `session::event` import at `:20`. `WindowId` is used only at `:288` (in `associate`) and `:1039` (the `extension_registry` test helper), and `window_map::self` only at `:294` (in `associate`); all three are deleted here.
    - Its test module: imports `crate::discovery::process_tree::FixedParents` (`:490`) and `crate::extension::registry::Registration` (`:496`); the helper `extension_registry()` (`:1039-1050`); and the two window tests that use it or `registry.window_of` — `a_scan_places_each_session_it_sees_in_the_window_running_it` (`.with_windows(..)` at `:1064`, `window_of` asserts at `:1072-1073`) and `a_scan_with_no_extension_involved_places_no_session_in_a_window` (`:1076-1091`).
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
  - Drop `WindowId` from the `session::event` import (`:20`) and `self` from the `window_map` import (`:16`), leaving `use crate::discovery::window_map::WindowMap;` only if `Scan.windows` stays (as `WindowMap::default()`); otherwise drop the whole `window_map` import. Clippy flags unused imports, so `Check` fails if either is left.
  - In the test module, delete the `FixedParents` and `Registration` imports, the `extension_registry()` helper and the two window tests named above. Do not rewrite them: window association is gone.
- `window_map.rs`: delete its whole `#[cfg(test)] mod tests` module. Do not rewrite it; 03_06 deletes the file. Leave the non-test code as is.
- `lib.rs`:
  - Remove `pub mod listener;` and `pub mod extension;`.
  - Remove the listener spawn and its state, `PORT_MEMORY_FILE`, `ListenerShutdown` and its exit trigger, `ExtensionRegistry`, the `with_windows` call, and the now-unused `SystemParents` import (`:23`).
- `Cargo.toml`: remove `axum` and `ureq`. Drop the tokio `net` feature only if nothing else needs it (run `cargo build` to check). `Cargo.lock` updates itself.

## Gotchas

- Grep `src-tauri/src` for `listener::`, `extension::`, `ExtensionPid`, `PortState`, `axum` and `ureq` after the change. All must be gone. `SystemParents` is not on this list: it stays defined in `discovery/process_tree.rs` until 03_06; only `lib.rs` and the reconciler stop using it. If one remains in a file outside this list, an earlier task missed it: stop and report rather than widening this task.
- `process_tree.rs` keeps `SystemParents` and `FixedParents` as unused `pub` items until 03_06 deletes it. That is expected; clippy does not flag `pub` items of a library crate.
- `test_support.rs` and `discovery/mod.rs` test helpers may reference listener types. Fix any you find.
- `RunEvent` exit handling must still trigger `ScanShutdown`, `ResumeShutdown`, `PublisherShutdown` and the `FeedShutdown` from 02_04.

## Verification

- `cargo test` and `cargo clippy` from `src-tauri/`, plus `cargo build` to confirm the dependency removal.
- `yarn test`.
- Substitute check that nothing listens any more, at test level only:
  - `grep -rnE 'TcpListener|TcpStream|\.bind\(' src-tauri/src` returns nothing (today every hit is under `listener/` and `extension/`).
  - `cargo tree -i axum` and `cargo tree -i ureq` (from `src-tauri/`) report that the package is not in the graph.
  - `cargo test file_feed` passes, which covers the file feed driving the registry snapshot (02_04's tests, including `apply_changes` producing an unread session).
- Do not launch the app (`yarn tauri dev` or any build), and do not quit or restart the owner's installed `/Applications/AYDY.app`. A dev launch would run 03_04's start-up cleanup against the owner's real `~/.claude/settings.json` (whose `X-Aydy-Hook` entries the installed app still depends on) and delete the real `removed-hooks.json`; `tauri-plugin-single-instance` (`src-tauri/src/lib.rs:78`) would likely hand the launch to the installed instance anyway, and `lsof` would always show the installed app listening. Do not create, edit, `touch` or delete anything under `~/Library/Application Support/app.aydy/`: touching a status file changes its recorded event time and makes that session unread in every window. The real-environment check (no TCP listener, popover follows real sessions) is the owner's at acceptance (spec, "Verification boundary").
