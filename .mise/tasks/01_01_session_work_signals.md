# 01_01 Session work signals

## Goal

Add the pure derivation of a Working session's stale and progress signals, and the per-session state it reads, to the Rust session model. Nothing feeds the new state yet; later tasks do.

## Files to modify/create

- `src-tauri/src/session/signals.rs` (new)
- `src-tauri/src/session/session.rs`
- `src-tauri/src/session/timing.rs`
- `src-tauri/src/session/mod.rs`

## Background

AYDY is a macOS menu bar app (Tauri v2, Rust core in `src-tauri/`) listing every Claude Code session. A session's status is derived from four booleans (`src-tauri/src/session/status.rs:47-79`) with a deliberate no-decay rule: a silent turn stays Working. This feature adds **stale** (Working but nothing written for 5 minutes) and **progress** (agent-reported ETA/step, else elapsed turn time; there is no TodoWrite checklist fallback — the transcript is read for its activity time only) as signals _beside_ status. Do not touch `StatusFacts`, `SessionStatus` or `rank()`.

The session model does no I/O and reads time only through the injected `Clock` as `Instant` (`src-tauri/src/clock.rs`); tests move a `TestClock`. Keep that: everything here is `Instant`/`Duration`.

The shared rule (spec `## Design`, identical in the VS Code extension later):

- `STALE_AFTER` = 5 minutes.
- Activity = newest of: transcript written at (incl. subagent transcripts — supplied by a later task as one `Instant`), the session's last hook event (`Session::last_event_at`, `session.rs:88-90`), the accepted progress report's `reported_at`. Transcript written-at unknown → `quiet_for` is `None` and the session is never stale.
- `quiet_for` = now − activity; `stale` = `quiet_for >= STALE_AFTER`.
- A progress report is accepted iff `reported_at >= turn_started_at` (no turn start → accepted). A rejected report contributes nothing (not step, not ETA, not activity).
- Step = accepted report's step, else `None`.
- ETA remaining = `eta − (now − reported_at)` in signed ms (negative = overdue); `reported_ago` = now − reported_at.
- `fraction_permille`: with an accepted ETA and a turn start: `elapsed * 1000 / (elapsed + max(0, remaining))`, floored, capped at 1000, and 1000 when the denominator is 0; else with a step: `done * 1000 / total`; else `None`.
- `turn_elapsed` = now − turn_started_at, or `None` without a turn start.
- Signals exist only while `status() == Working`.

Patterns: `SessionTiming` (`timing.rs:13-70`) for a small `Copy`/`Eq` state type with doc comments; `Session` setters such as `mark_present` (`session.rs:203-207`); test style in `session.rs:262+` (`minutes(n)` helper, `Instant::now()` base, one behaviour per test, descriptive snake-case names).

## Guides

None of the config's Skills & guides entries apply (no Vue, no popover layout).

## Implementation details

1. `timing.rs`: add `pub fn turn_started_at(&self) -> Option<Instant>` returning the private `turn_started_at`.
2. `signals.rs` (module doc comment explaining stale is beside status, not a status):
   - `pub const STALE_AFTER: Duration = Duration::from_secs(5 * 60);`
   - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Step { pub done: u32, pub total: u32 }` with `pub fn new(done, total) -> Option<Step>` returning `None` unless `total >= 1 && done <= total`.
   - `pub struct ProgressReport { pub reported_at: Instant, pub eta: Duration, pub step: Option<Step> }`.
   - `pub struct Eta { pub remaining_ms: i64, pub reported_ago: Duration }`.
   - `pub struct WorkSignals { pub stale: bool, pub quiet_for: Option<Duration>, pub turn_elapsed: Option<Duration>, pub step: Option<Step>, pub eta: Option<Eta>, pub fraction_permille: Option<u16> }`.
   - `pub struct SignalInputs { pub turn_started_at: Option<Instant>, pub last_event_at: Instant, pub transcript_written_at: Option<Instant>, pub progress: Option<ProgressReport> }`.
   - `pub fn derive(inputs: SignalInputs, now: Instant) -> WorkSignals` implementing the rule. Use `saturating_duration_since` everywhere; convert durations to i64 ms saturating.
3. `mod.rs`: `pub mod signals;`.
4. `session.rs`: private fields `transcript_written_at: Option<Instant>`, `progress: Option<ProgressReport>` (initialised `None` in `new`). Public methods:
   - `record_transcript(&mut self, written_at: Instant)` — keeps the later of the stored and new written-at (an older reading must not move activity backwards).
   - `record_progress(&mut self, report: ProgressReport)` — replaces the stored report.
   - `work_signals(&self, now: Instant) -> Option<WorkSignals>` — `None` unless Working; else `signals::derive(...)` with `self.timing.turn_started_at()` and `self.last_event_at`.
     None of these call `settle` or change `last_event_at` (they are not events and must not reorder the list).

## Gotchas

- Do not make stale a status or feed it into `settle`; the working clock and Unknown window must be unaffected.
- `turn_started_at` is `None` after `end_turn`; a NeedsInput mid-turn keeps it, but signals are only produced while Working anyway.
- The turn start is the spec's shared rule, and the VS Code extension mirrors exactly what `SessionTiming` does today (task 03_02 `nextTurnStart`): `UserPromptSubmit` and `SubagentsRunning` call `start_turn`, which keeps an open turn's start (`timing.rs:40-44`); `Stop` calls `end_turn`; `Notification` leaves it (`session.rs:111-144`). Do not change those semantics here.
- `cargo clippy` runs in `Check`; avoid `as` casts that clippy flags — use `try_from(...).unwrap_or(...)`.
- The new setters have no production caller until 01_03/01_04. They are `pub` on a `pub` type in a `pub mod`, so the dead-code lint does not fire; keep them `pub` rather than adding `#[allow(dead_code)]`.

## Verification

- Run `cargo test` and `cargo clippy` from `src-tauri/`.
- Write tests in `signals.rs` (`#[cfg(test)] mod tests`):
  - quiet for under 5 minutes is not stale; exactly 5 minutes is stale;
  - unknown transcript written-at gives `quiet_for == None` and never stale, however old the last event;
  - a hook event newer than the transcript resets quiet;
  - an accepted report counts as activity;
  - a report written before the turn started is ignored (no step, no ETA, no activity);
  - the accepted report's step is the step; no accepted report → no step;
  - ETA remaining counts down and goes negative past the ETA;
  - fraction from ETA, from step, and `None` with neither;
  - `Step::new` rejects `total == 0` and `done > total`.
- Write tests in `session.rs`: after `UserPromptSubmit` then `SubagentsRunning` (Stop with subagents still running), a report written between the two is accepted and one written before the prompt is rejected (the turn start stays the prompt's); `work_signals` is `None` for Done / NeedsInput / Unknown and `Some` while Working; `record_transcript` with an older instant does not move written-at backwards; recording progress or transcript does not change `last_event_at`.
