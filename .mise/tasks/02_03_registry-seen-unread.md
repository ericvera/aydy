# 02_03 Unread from attention vs seen times

## Goal

Make the app's unread state come from the shared seen files rather than from VS Code focus reports. A session is unread when its last `Stop` or `Notification` is newer than its last seen time. Marking a row read in the popover writes the seen file, so the VS Code tab count agrees. Sounds stay quiet for sessions that are already seen.

## Files to modify/create

- src-tauri/src/session/registry.rs
- src-tauri/src/session/session.rs
- src-tauri/src/notify/sound.rs
- src-tauri/src/ipc/commands.rs
- src-tauri/src/lib.rs

## Background

- Task 02_02 produced, in `src-tauri/src/discovery/status_files.rs`: `StatusRecord { event: HookEvent, written_at_ms: u64 }`, `DataDir` (`seen_dir()`) and `write_seen(dir, id, ms)`.
- How unread works today:
  - `Session.unread: bool` (`session/session.rs:37`).
  - It is set in `SessionRegistry::apply_hook_event` (`session/registry.rs:186-219`) for Stop and Notification unless `is_watched` (`:675`).
  - It is cleared by `read_window` (`:407`), by `apply_focus_report` (`:497-507`) and by `mark_read` (`:533`).
  - `SessionSnapshot` (`:36`) carries `unread`, and `RegistrySnapshot.unread_count` is derived from it.
- Owner decision 1: a session whose Stop or Notification arrives while its window has focus is not counted, and arriving at a window clears it. The VS Code extension now enforces this by writing seen files, so the app only compares times.
- Sounds: `SoundNotifier::observe` (`notify/sound.rs:213-245`) skips `session.is_current` (`:238`).
- `mark_read` command: `ipc/commands.rs:178-189` (`mark_read`, `mark_session_read`).
- App setup: `lib.rs:119-240`. `app.path().app_data_dir()` is `<data>`.
- This task leaves `apply_hook_event`, the focus and window functions, and `is_current` in place. The listener still calls them until stage 3, and they are deleted in 03_06.

## Guides

None match.

## Implementation details

- `session.rs`:
  - Replace `pub unread: bool` with `pub attention_at_ms: Option<u64>` and `pub seen_at_ms: Option<u64>`.
  - Add `pub fn unread(&self) -> bool { matches!((self.attention_at_ms, self.seen_at_ms.unwrap_or(0)), (Some(a), s) if a > s) }`.
  - Update every use.
- `registry.rs`:
  - `pub fn apply_status(&mut self, record: StatusRecord)` applies the event through the same path as `apply_hook_event` (factor out a private `apply_event`). For Stop and Notification it also sets `attention_at_ms = Some(record.written_at_ms)`. SessionEnd removes the session as today.
  - `pub fn apply_seen(&mut self, session_id: &str, seen_at_ms: u64)` keeps the max, and does nothing for unknown sessions.
  - `pub fn mark_read(&mut self, session_id: &str, now_ms: u64)` sets `seen_at_ms = max(seen, now_ms)`.
  - Remove the `notifies && !watched` unread setting from `apply_hook_event`. Events arriving there no longer touch unread.
  - Delete `read_window` and the unread clear in `apply_focus_report`, and delete `is_watched` if it becomes unused.
  - The snapshot uses `session.unread()`.
  - Update the doc comments that describe the old rule.
- `notify/sound.rs`: replace the `is_current` skip with `if !session.unread { continue; }` and rewrite its comment. A session already seen when its transition is observed is one the user is looking at.
- `ipc/commands.rs`:
  - `mark_read` takes `data: State<'_, DataDir>` as well.
  - `mark_session_read(registry, data, id, now_ms)` updates the registry, then calls `write_seen`. On failure it logs with `eprintln!("aydy: ...")` and does not fail the command.
  - `now_ms` comes from `SystemTime::now()`.
  - Switch its `lock_recovering` import to `crate::sync`.
- `lib.rs`: `app.manage(DataDir::new(app.path().app_data_dir()?))`.

## Gotchas

- Wall-clock ms (status file mtime, seen time) is never compared against the registry's `Instant` clock. Keep the two apart.
- The registry tests use `apply_hook_event` heavily for unread. Rewrite the unread tests against `apply_status`, `apply_seen` and `mark_read`. Delete tests for focus-driven unread, since that behaviour is removed on purpose.
- `SessionSnapshot.unread` stays a `bool` field, so the frontend contract is unchanged.

## Verification

- In `registry.rs`, add or replace tests:
  - Stop via `apply_status` is unread, and an `apply_seen` older than it stays unread while a newer one clears it.
  - Notification behaves the same way.
  - SessionStart and UserPromptSubmit are never unread.
  - `mark_read` clears it, and a later Stop raises it again.
  - `unread_count` matches.
- In `sound.rs`, a transition to Done for a session that is not unread plays nothing, and one for an unread session plays.
- In `commands.rs`, `mark_session_read` writes `seen/<id>.json` in a scratch `DataDir`.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
