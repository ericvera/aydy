# 02_02 Shared hook payload parsing and status/seen file reader

## Goal

Give the app a reader for the per-session status files the hook script writes and the seen files the extension writes. It reuses the payload parsing the HTTP listener already has.

## Files to modify/create

- src-tauri/src/session/hook_payload.rs (new)
- src-tauri/src/session/mod.rs
- src-tauri/src/listener/routes.rs
- src-tauri/src/discovery/status_files.rs (new)
- src-tauri/src/discovery/mod.rs

## Background

- Stage 1 (VS Code extension) installs `<data>/bin/aydy-hook.sh` as a hook. `<data>` is `~/Library/Application Support/app.aydy`, which is `app.path().app_data_dir()` for identifier `app.aydy` (`src-tauri/tauri.conf.json:5`). Its files:
  - `<data>/status/<session_id>.json` is the raw hook payload. The file's mtime is the event time. Dotfiles in that dir are temp files.
  - `<data>/seen/<session_id>.json` is `{"seenAt": <epoch ms>}`.
- The payload parsing exists in `src-tauri/src/listener/routes.rs:41-90`:
  - `HookPayload`, whose fields are optional and `serde(default)`.
  - `into_event`, which maps `SessionStart`, `UserPromptSubmit`, `Notification`, `Stop` and `SessionEnd` to `HookEventKind` (`src-tauri/src/session/event.rs:22-44`).
  - `present`, which treats blank strings as absent.
- The atomic writer is `src-tauri/src/atomic_file.rs:29` (`write_atomically(path, contents)`).
- Test scratch dirs: `src-tauri/src/test_support.rs` (`scratch_dir`) and `src-tauri/src/discovery/mod.rs:327-364`.

## Guides

None match.

## Implementation details

- `session/hook_payload.rs` takes `HookPayload`, `into_event` and `present` moved verbatim from `routes.rs`, together with their tests. In `session/mod.rs`, add `pub mod hook_payload;`. `routes.rs` imports from there.
- `discovery/status_files.rs`:
  - `pub struct DataDir { root: PathBuf }` with `new(root)`, `status_dir()`, `seen_dir()`, `jump_dir()` and `hook_script()` (`<root>/bin/aydy-hook.sh`).
  - `pub struct StatusRecord { pub event: HookEvent, pub written_at_ms: u64 }`, where `session_id` lives in `event`.
  - `pub fn read_statuses(dir: &Path) -> Vec<StatusRecord>`. It skips dotfiles, non-`.json` files, unreadable or unparseable files, and payloads with no event. It requires the payload's `session_id` to equal the file stem. The mtime comes from `metadata.modified()` as epoch ms. A missing dir gives an empty Vec.
  - `pub fn read_seen(dir: &Path) -> HashMap<String, u64>`, which skips bad files.
  - `pub fn write_seen(dir: &Path, session_id: &str, seen_at_ms: u64) -> io::Result<()>` runs `create_dir_all`, then `write_atomically`. It rejects ids that do not match `^[A-Za-z0-9_-]{1,128}$` with `InvalidInput`.
- In `discovery/mod.rs`, add `pub mod status_files;` and extend its module doc with one line on the new second source.

## Gotchas

- Nothing here may be fatal or panic. Every bad file is skipped, as in `discovery/mod.rs:312-315`.
- Keep `routes.rs` behaviour identical. Its tests must pass unchanged, apart from imports.
- A status file whose stem differs from its `session_id` is skipped. That keeps a rename or a stray file from creating a phantom session.

## Verification

- The moved `HookPayload` tests keep passing in `hook_payload.rs`.
- New tests in `status_files.rs`, using scratch dirs:
  - each event kind parses with the right `written_at_ms` (set the mtime with `File::set_modified`)
  - dotfiles, a bad file and a stem mismatch are skipped
  - a missing dir gives empty results
  - the seen round-trip works
  - `write_seen` rejects `../x`
  - the `DataDir` paths are correct
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
