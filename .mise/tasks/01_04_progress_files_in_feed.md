# 01_04 Progress files in the feed

## Goal

Read the agent-reported progress files every half second and apply them to their sessions, so a Working session's signals carry the agent's ETA and step.

## Files to modify/create

- `src-tauri/src/discovery/status_files.rs`
- `src-tauri/src/discovery/file_feed.rs`
- `src-tauri/src/session/registry.rs`

## Background

AYDY (Tauri v2 menu bar app, Rust core in `src-tauri/`) polls its data directory (`~/Library/Application Support/app.aydy/`) every 0.5 s in `FileFeed::pass` (`file_feed.rs:77-137`), believing a session's files only while its Claude process is alive (`is_live`, `:139-141`), and applies changes under one lock in `apply_changes` (`:185-199`). Status records reach the registry as of when they were written: `apply_status(record, now_ms)` converts the file's mtime age to an `Instant` (`registry.rs:166-188`).

A new Claude Code plugin skill (task 03_01) writes one progress file per session. The contract (spec `## Design`):

- Path `app.aydy/progress/<session-id>.json`, written atomically; the file's **mtime is the report time**.
- Body `{"etaSeconds":360,"step":{"done":4,"total":7}}`; `step` optional.
- Skip a file whose stem is not a valid session id (`is_valid_session_id`, `status_files.rs:126-131`; dotfiles are already skipped by `json_files`, `:134-148`), whose JSON is unparseable, or lacks a non-negative integer `etaSeconds`. Drop `step` (keep the report) unless `1 <= total` and `0 <= done <= total`.

Prior tasks produced: `crate::session::signals::{ProgressReport { reported_at, eta, step }, Step::new}` and `Session::record_progress(report)` (01_01, `src-tauri/src/session/signals.rs`, `session.rs`). The "report older than the turn start is discarded" rule already lives in `signals::derive`; this task only delivers reports.

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `status_files.rs`:
   - `DataDir::progress_dir()` → `root/progress`.
   - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct ProgressRecord { pub session_id: String, pub eta: Duration, pub step: Option<Step>, pub written_at_ms: u64 }`.
   - `pub fn read_progress(dir: &Path) -> Vec<ProgressRecord>` following `read_statuses` (`:64-86`): mtime via `epoch_ms`, body via a private `#[derive(Deserialize)] #[serde(rename_all = "camelCase")] struct ProgressFile { eta_seconds: u64, step: Option<StepFile> }` with `StepFile { done: u32, total: u32 }` — but parse leniently so a bad `step` costs the step only: deserialize into `serde_json::Value` and pick fields, or make `step: Option<serde_json::Value>`. Skip invalid stems.
   - Cap `eta` at 24 h when reading (the command caps too; a hand-written file must not overflow arithmetic).
2. `file_feed.rs`:
   - `FeedChanges` gains `pub progress: Vec<ProgressRecord>`.
   - `FileFeed` gains `progress_applied: HashMap<String, u64>` (session → mtime last forwarded). In `pass`, read `read_progress(&self.data.progress_dir())`; forward a record when its session `is_live` and its mtime differs from `progress_applied`; drop entries for files no longer present. No hold logic (progress asks for no attention).
   - `apply_changes`: after status records and before `ended`, `registry.apply_progress(record, now_ms)` for each.
3. `registry.rs`: `pub fn apply_progress(&mut self, record: ProgressRecord, now_ms: u64)` — ignore unknown session ids (a progress file never creates a session); convert `written_at_ms` to an `Instant` exactly as `apply_status` does; call `session.record_progress(ProgressReport { reported_at, eta: record.eta, step: record.step })`.

## Gotchas

- Apply progress **after** status records in the same pass: a `UserPromptSubmit` and an older progress file read together must leave the report rejected by the turn-start rule, which needs the turn opened first.
- A future mtime (clock skew) has no age and lands at now — same as `apply_status`.
- `FeedChanges` derives `PartialEq` and tests compare it; existing test literals may need `progress: Vec::new()` or `..Default::default()`.
- The first pass forwards every present progress file (no hold), so a relaunch mid-turn picks up the current ETA; the turn-start rule rejects reports from before an adopted turn began, which is accepted (spec).

## Verification

- Run `cargo test` and `cargo clippy --all-targets -- -D warnings` from `src-tauri/`.
- New tests:
  - `status_files.rs`: reads eta and step with the file's mtime; a missing `step` is fine; an invalid step (`done > total`, `total == 0`) is dropped but the report kept; missing or negative `etaSeconds`, bad JSON and an invalid stem are skipped; eta above 24 h is capped.
  - `file_feed.rs` (use the existing `Fixture` helpers, `:330-405`; add a `progress(session_id, body, written_at_ms)` helper): a live session's progress is forwarded once and again only after its mtime changes; a dead session's progress is not forwarded; after `apply_changes` a Working session's snapshot `signals` carry the ETA and an agent step; a progress file older than the prompt applied in the same pass yields no ETA.
  - `registry.rs`: `apply_progress` for an unknown session creates nothing.
