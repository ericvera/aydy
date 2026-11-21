# 03_05 Delete the HTTP listener and the extension registry/channel

## Goal

The app stops serving the loopback HTTP listener and stops tracking VS Code windows through extension registrations. It reads files only.

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

- What happened in earlier tasks:
  - 02_04 moved hook events to `discovery/file_feed.rs`.
  - 02_05 moved jump to request files.
  - 03_03 removed health from the snapshot and the extension-status command.
  - 03_04 removed the installer and port file.
  - `lock_recovering` lives in `src-tauri/src/sync.rs`. `listener/server.rs` only re-exports it, so any remaining importer must switch to `crate::sync`.
- What remains:
  - `lib.rs`: the listener spawn (`listener::server::run(memory, ports, registry, extensions, ...)`), `PortMemory` and `PortState`, `PORT_MEMORY_FILE`, `ExtensionRegistry`, `ExtensionCommands` and `LoopbackChannel` if still present, `ListenerShutdown` and its trigger on exit, and `Reconciler::with_windows(windows, Arc::new(SystemParents))` (`lib.rs:229-230`).
  - `discovery/reconciler.rs`: `with_windows(Arc<ExtensionRegistry>, Arc<dyn ProcessParents>)` (`:143`) and `associate` (`:283-296`), which calls `window_map::associate(live, registration pids, parents)`. Also its imports at `:12`, `:17` and `:18`.
  - `discovery/window_map.rs`: its tests import `ExtensionRegistry` (`~:103`).
  - `session/event.rs:5` and `:20`: `pub type WindowId = ExtensionPid`, imported from `extension::registry`.
- Dependencies: `axum` is used only by the listener. `ureq` is used only by `extension/command.rs`'s `LoopbackChannel`. The tokio `net` feature may be needed only by the listener.

## Guides

None match.

## Implementation details

- `event.rs`: `pub type WindowId = u32;`, with the import dropped. Window association itself is removed in 03_06, so keep the alias for now.
- `reconciler.rs`:
  - Delete `with_windows`, the `extensions` and `parents` fields and `associate`. `Scan.windows` becomes `WindowMap::default()`, or remove the field if nothing reads it. 03_06 deletes `WindowMap` entirely, so prefer the smallest edit that compiles.
  - Switch `lock_recovering` to `crate::sync`.
- `window_map.rs`: rewrite the tests that built an `ExtensionRegistry` to use plain pids.
- `lib.rs`:
  - Remove `pub mod listener;` and `pub mod extension;`.
  - Remove the listener spawn and its state, `PORT_MEMORY_FILE`, `ListenerShutdown` and its exit trigger, `ExtensionRegistry` and the `with_windows` call.
- `Cargo.toml`: remove `axum` and `ureq`. Drop the tokio `net` feature only if nothing else needs it (run `cargo build` to check). `Cargo.lock` updates itself.

## Gotchas

- Grep `src-tauri/src` for `listener::`, `extension::`, `ExtensionPid`, `PortState`, `axum` and `ureq` after the change. All must be gone.
- `test_support.rs` and `discovery/mod.rs` test helpers may reference listener types. Fix any you find.
- `RunEvent` exit handling must still trigger `ScanShutdown`, `ResumeShutdown`, `PublisherShutdown` and the `FeedShutdown` from 02_04.

## Verification

- `cargo test` and `cargo clippy` from `src-tauri/`, plus `cargo build` to confirm the dependency removal.
- `yarn test`.
- Substitute check, run against the app's real port: launch the app (`yarn tauri dev`) and confirm nothing listens on loopback for it (`lsof -iTCP -sTCP:LISTEN -P | grep -i aydy` is empty). Also confirm that writing a status file for a live session makes it appear in the popover. Quit the app afterwards.
