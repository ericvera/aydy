# 02_03 Unread from attention vs seen times

## Goal

Make the app's unread state come from the shared seen files rather than from VS Code focus reports. A session is unread when its last `Stop` or `Notification` is newer than its last seen time. Status records are applied at the time they were recorded, not at the time the app read them. Marking a row read in the popover writes the seen file, so the VS Code tab count agrees. Sounds stay quiet for sessions that are already seen, and stay quiet until the notifier is armed, so state loaded at startup never plays a sound.

## Files to modify/create

- src-tauri/src/session/registry.rs
- src-tauri/src/session/session.rs
- src-tauri/src/notify/sound.rs
- src-tauri/src/ipc/commands.rs
- src-tauri/src/lib.rs

## Background

- Task 02_02 produced, in `src-tauri/src/discovery/status_files.rs`: `StatusRecord { event: HookEvent, written_at_ms: u64 }` (`written_at_ms` is the status file's mtime, the event's recorded time), `DataDir` (`seen_dir()`) and `write_seen(dir, id, ms)`.
- Task 02_01 moved `lock_recovering` to `crate::sync`.
- How unread works today:
  - `Session.unread: bool` (`session/session.rs:37`).
  - It is set in `SessionRegistry::apply_hook_event` (`session/registry.rs:186-219`) for Stop and Notification unless `is_watched` (`:675`).
  - It is cleared by `read_window` (`:407`), by `apply_focus_report` (`:475`) and by `mark_read` (`:533`).
  - `SessionSnapshot` (`:36`) carries `unread`, and `RegistrySnapshot.unread_count` is derived from it.
- Time today: `apply_hook_event` stamps every event with `self.clock.now()` (`:187`), a monotonic `Instant`. The snapshot sorts by `last_event_at` (`:597`) and working time is measured from those instants. Task 02_04 replays every live status file on its first pass, so stamping records with `now` would sort yesterday's `Stop` as brand new and count a `UserPromptSubmit`'s working time from app launch.
- Owner decision 1: a session whose Stop or Notification arrives while its window has focus is not counted, and arriving at a window clears it. The VS Code extension now enforces this by writing seen files, so the app only compares times.
- Sounds:
  - `SoundNotifier::observe` (`notify/sound.rs:213-245`) skips `session.is_current` (`:238`).
  - `transition_alert` (`notify/mod.rs:31-40`) gives nothing for a first sighting, but a session first seen as `Unknown` by the scan and then set to `Done` by a replayed status file is a transition and would play at launch.
  - `sound.rs:20` imports `lock_recovering` from `crate::listener::server`.
  - The test module imports `FocusReport` and `WindowId` (`:353`), defines `const WINDOW` (`:357`), has `Harness::focus` calling `apply_focus_report` (`:402-407`), and seven tests built on it (`:634-740`): the two "active in a focused window stays silent" tests, the two "active in an unfocused window still sounds" tests, "background tab of a focused window still sounds", and the two "suppressed transition" tests. `Harness::hook` drives `apply_hook_event`.
- `mark_read` command: `ipc/commands.rs:178-189` (`mark_read`, `mark_session_read`). Its test helper `is_unread` reads `.unread`.
- App setup: `lib.rs:119-240`. `app.path().app_data_dir()` is `<data>`. `SoundNotifier::new` is called at `lib.rs:161`.
- This task leaves `apply_hook_event`, the focus and window functions, and `is_current` in place. The listener still calls them until stage 3, and they are deleted in 03_06 and 03_07.

## Guides

None match.

## Implementation details

- `session.rs`:
  - Replace `pub unread: bool` with `pub attention_at_ms: Option<u64>` and `pub seen_at_ms: Option<u64>`.
  - Add `pub fn unread(&self) -> bool { matches!((self.attention_at_ms, self.seen_at_ms.unwrap_or(0)), (Some(a), s) if a > s) }`.
  - Update every use.
- `registry.rs`:
  - Factor the body of `apply_hook_event` into a private `apply_event(&mut self, event: HookEvent, at: Instant)`. `apply_hook_event` calls it with `self.clock.now()`.
  - `pub fn apply_status(&mut self, record: StatusRecord, now_ms: u64)`:
    1. `let now = self.clock.now(); let age = Duration::from_millis(now_ms.saturating_sub(record.written_at_ms)); let at = now.checked_sub(age).unwrap_or(now);`
    2. `apply_event(record.event, at)`, so the session's event time, its sort order and any working time start at the recorded time.
    3. For Stop and Notification, also set `attention_at_ms = Some(record.written_at_ms)`.
    4. SessionEnd removes the session as today.
  - `pub fn apply_seen(&mut self, session_id: &str, seen_at_ms: u64)` keeps the max, and does nothing for unknown sessions.
  - `pub fn mark_read(&mut self, session_id: &str, now_ms: u64)` sets `seen_at_ms = max(seen, now_ms)`.
  - Remove the `notifies && !watched` unread setting from `apply_hook_event`. Events arriving there no longer touch unread.
  - Delete `read_window` and the unread clear in `apply_focus_report`, and delete `is_watched` if it becomes unused.
  - The snapshot uses `session.unread()`.
  - Update the doc comments that describe the old rule.
- `notify/sound.rs`:
  - Switch the `lock_recovering` import to `crate::sync`.
  - Replace the `is_current` skip with `if !session.unread { continue; }` and rewrite its comment. A session already seen when its transition is observed is one the user is looking at.
  - Arming: `NotifierState` gains `armed: bool` and `baseline_pending: bool`, both false at construction. `pub fn arm(&self)` sets `baseline_pending = true` (a no-op when already armed). In `observe`, always rebuild `state.previous` as today, but earn nothing while not armed. When `baseline_pending` is set, that observation is the baseline: it earns nothing, then sets `armed = true` and clears `baseline_pending`.
  - Rewrite the module and `observe` docs: sounds play only for unread sessions, and only for changes after the initial load.
- `ipc/commands.rs`:
  - `mark_read` takes `data: State<'_, DataDir>` as well.
  - `mark_session_read(registry, data, id, now_ms)` updates the registry, then calls `write_seen`. On failure it logs with `eprintln!("aydy: ...")` and does not fail the command.
  - `now_ms` comes from `SystemTime::now()`.
  - Switch its `lock_recovering` import to `crate::sync`.
- `lib.rs`:
  - `app.manage(DataDir::new(app.path().app_data_dir()?))`.
  - Call `sounds.arm()` right after `SoundNotifier::new`. Nothing loads from files yet, so this keeps today's behaviour. Task 02_04 moves the call to after the feed's first pass.

## Gotchas

- Wall-clock ms (status file mtime, seen time, `now_ms`) is never compared against the registry's `Instant` clock directly. The only bridge is the age subtraction in `apply_status`. A record dated in the future (clock skew) has age 0 and lands at `now`.
- If a `Session` timing field must stay monotonic (for example a session the scan already marked present at `now` receiving an older recorded event), keep that one field at the max of the two instants rather than moving it backwards. Do not clamp the event time itself to `now`.
- The registry tests use `apply_hook_event` heavily for unread. Rewrite the unread tests against `apply_status`, `apply_seen` and `mark_read`. Delete tests for focus-driven unread, since that behaviour is removed on purpose.
- In `sound.rs` tests, delete the `FocusReport`/`WindowId` imports, `const WINDOW`, `Harness::focus` and the seven focus tests listed above. They test a rule this task removes, and `apply_focus_report` is deleted in 03_06. Make `Harness::hook` feed Stop and Notification through `apply_status` (with a `written_at_ms` that advances per call and a matching `now_ms`) so they are unread, and have `Harness::new` call `arm()` and run one baseline `observe()` so the existing transition tests keep their meaning.
- Sounds must skip a session that is already seen when its transition is observed, and only that. Task 02_04's feed holds a new `Stop` or `Notification` for one 500 ms pass so the extension's seen write (about 150 ms after the status file) is applied together with it; that timing lives entirely in the feed, so do not add a delay or hold to `SoundNotifier`.
- `SessionSnapshot.unread` stays a `bool` field, so the frontend contract is unchanged. Do not add fields to `SessionSnapshot`: `tray/icon.rs` and `ipc/snapshot_channel.rs` build it in their tests.

## Verification

- In `registry.rs`, add or replace tests:
  - Stop via `apply_status` is unread, and an `apply_seen` older than it stays unread while a newer one clears it.
  - Notification behaves the same way.
  - SessionStart and UserPromptSubmit are never unread.
  - `mark_read` clears it, and a later Stop raises it again.
  - `unread_count` matches.
  - Recorded time: two sessions whose Stop records are applied in the order newer-then-older (`written_at_ms` 1 h ago and 2 h ago, same `now_ms`) sort with the newer one first; a `UserPromptSubmit` recorded 10 minutes before `now_ms` shows about 10 minutes of working time immediately.
- In `sound.rs`:
  - a transition to Done for a session that is not unread (seen newer than the Stop) plays nothing, and one for an unread session plays
  - a session seen after its Stop stays silent, and its next unread Notification sounds
  - before `arm()`, an `Unknown` to `Done` transition of an unread session plays nothing; the first observation after `arm()` plays nothing; a transition after that plays
- In `commands.rs`, `mark_session_read` writes `seen/<id>.json` in a scratch `DataDir`.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
