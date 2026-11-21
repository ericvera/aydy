# 02_01 Move `lock_recovering` into `sync.rs`

## Goal

Move the shared mutex helper `lock_recovering` out of the HTTP listener and into its own module. The listener is deleted in stage 3, and everything that locks the registry needs the helper to survive that.

## Files to modify/create

- src-tauri/src/sync.rs (new)
- src-tauri/src/lib.rs
- src-tauri/src/listener/server.rs
- src-tauri/src/discovery/resume.rs
- src-tauri/src/tray/popover.rs

## Background

- `lock_recovering<T>(value: &Mutex<T>) -> MutexGuard<'_, T>` is defined at `src-tauri/src/listener/server.rs:118`, along with its doc comment and any tests that cover it in that file.
- Its importers outside the listener are:
  - `discovery/reconciler.rs:18`
  - `discovery/resume.rs:42`
  - `jump/mod.rs:34`
  - `notify/sound.rs:20`
  - `tray/popover.rs:9`
  - `ipc/snapshot_channel.rs:24`
  - `ipc/commands.rs:23`
  - inside modules that are deleted later: `extension/registry.rs:9`, `extension/command.rs:330` (test module) and `hooks/install.rs:20`. These stay on the re-export until their files are deleted (03_05 and 03_04).
- Which task switches each remaining importer: 02_03 (`notify/sound.rs`, `ipc/commands.rs`), 02_05 (`jump/mod.rs`), 03_03 (`ipc/snapshot_channel.rs`), 03_05 (`discovery/reconciler.rs`).
- Plan (`.mise/spec.md`, "App"): the server re-exports the helper so the remaining importers keep compiling. Every later task that touches one of them switches its import to `crate::sync::lock_recovering`. Listener deletion (03_05) removes the re-export.

## Guides

None match.

## Implementation details

- Create `src-tauri/src/sync.rs` with a module doc comment, the function moved verbatim, and its tests.
- In `lib.rs`, add `pub mod sync;` in alphabetical order with the other modules (`lib.rs:1-15`).
- In `listener/server.rs`, replace the definition with `pub use crate::sync::lock_recovering;` and keep its own internal uses working.
- Switch the imports in `discovery/resume.rs` and `tray/popover.rs` to `crate::sync::lock_recovering`.

## Gotchas

- Do not change behaviour. This is a pure move.
- `cargo clippy` must stay clean. A re-export used by other modules raises no unused warning, but check it.

## Verification

- Move the existing tests for `lock_recovering` with it. If there are none, add one to `sync.rs`: a mutex poisoned by a panicking thread still yields its value.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
- Run `yarn test` as well, to keep the full suite green.
