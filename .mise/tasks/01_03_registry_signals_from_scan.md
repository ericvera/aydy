# 01_03 Registry signals from the scan

## Goal

Carry each scanned session's transcript activity and checklist into the registry, and put the derived work signals on every session snapshot.

## Files to modify/create

- `src-tauri/src/session/event.rs`
- `src-tauri/src/session/registry.rs`
- `src-tauri/src/discovery/reconciler.rs`

## Background

AYDY (Tauri v2 menu bar app, Rust core in `src-tauri/`) runs a reconciliation scan every 25 s (`reconciler.rs:42`, `Reconciler::scan` `:145-244`) that reads each live session's transcript and hands enrichment to the registry through `enrichment(scan)` (`:319-354`) and `SessionRegistry::apply_enrichment` (`registry.rs:270-292`). The registry runs on an injected monotonic clock; wall-clock times reach it only as an _age_, converted the way `apply_status` does (`registry.rs:166-176`: `at = now.checked_sub(age).unwrap_or(now)`).

Prior tasks produced:

- 01_01: `crate::session::signals::{Step, WorkSignals, STALE_AFTER, …}`; `Session::record_transcript(written_at: Instant, checklist: Option<Step>)`, `Session::work_signals(now) -> Option<WorkSignals>` (`src-tauri/src/session/session.rs`, `signals.rs`).
- 01_02: `TranscriptFacts.checklist: Option<Step>` and `transcript::last_activity(path, session_id) -> Option<SystemTime>` (`src-tauri/src/discovery/transcript.rs`).

Design decisions reaching this task: activity is uncached stats every scan; the registry stays on `Instant`; signals are computed when the snapshot is taken (`registry.rs:342-414`), so `quiet_for`, `turn_elapsed` and ETA remaining advance on every 100 ms observation, not only every scan.

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `event.rs`: add
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub struct TranscriptActivity { pub written_ago: Duration, pub checklist: Option<Step> }
   ```
   and `pub transcript: Option<TranscriptActivity>` on `SessionEnrichment` (still `Default`). Doc: `None` means the scan found no readable transcript, which changes nothing.
2. `reconciler.rs`:
   - `Scan` gains `pub activity: Vec<(SessionId, SystemTime)>`. In `scan()`, where a transcript is located (`:174-186`), also call `transcript::last_activity(&path, &session.session_id)` and push it when `Some`.
   - `enrichment(scan: &Scan, now: SystemTime)`; `scan_and_apply` passes `SystemTime::now()`. For each present session with an activity time: `written_ago = now.duration_since(time).unwrap_or(Duration::ZERO)` (a future mtime has no age), and `checklist` from that session's `TranscriptFacts`. Set `transcript: Some(TranscriptActivity { … })`; otherwise `None`.
3. `registry.rs`:
   - `apply_enrichment`: `if let Some(activity) = enrichment.transcript { let at = now.checked_sub(activity.written_ago).unwrap_or(now); session.record_transcript(at, activity.checklist); }` with `now = self.clock.now()`.
   - `SessionSnapshot` gains `pub signals: Option<WorkSignals>`, filled with `session.work_signals(now)` in `snapshot()`.
   - Ordering, grouping, counts: unchanged.
4. Update every `SessionSnapshot { … }` literal in tests so the crate compiles: `src-tauri/src/tray/icon.rs:100-113` and `src-tauri/src/ipc/snapshot_channel.rs` tests (`:400-415`, `:477`, `:511`) get `signals: None`. These are test edits only.

## Gotchas

- `SessionEnrichment` literals in tests mostly use `..SessionEnrichment::default()`; `ipc/commands.rs:200` and `registry.rs` tests too — check `cargo test` compiles everywhere.
- `enrichment()` is called from tests in `reconciler.rs`; update their calls with a fixed `SystemTime` so ages are deterministic.
- A scan must never reorder the list: `record_transcript` does not touch `last_event_at` (01_01 guarantees it); do not add calls that do.
- `SessionSnapshot` derives `Eq`; `WorkSignals` already does.

## Verification

- Run `cargo test` and `cargo clippy --all-targets -- -D warnings` from `src-tauri/`.
- New tests:
  - `reconciler.rs`: a scan of a live session with a transcript reports its activity; `enrichment` turns it into `written_ago` against the supplied `now` and carries the checklist; a session with no transcript gets `transcript: None`.
  - `registry.rs` (with `TestClock`): a Working session enriched with `written_ago = 6 min` snapshots as stale with `quiet_for ≈ 6 min`; after a fresh `UserPromptSubmit` status record it is no longer stale; a Done session's snapshot has `signals: None`; advancing the clock without a new scan makes a quiet Working session turn stale on the next snapshot; enrichment does not change the session's position in its group.
