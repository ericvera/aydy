# 03_02 Extension: signal derivation and store reads

## Goal

Give the VS Code companion extension its own copy of the shared stale/progress rule — pure functions plus the file reads behind them — so a later task can put the signal in the window title.

## Files to modify/create

- `extension/src/workSignal.ts` (new)
- `extension/src/sessionState.ts`
- `extension/src/sessionStore.ts`
- `extension/src/aydyPaths.ts`
- `extension/src/extension.ts` (one line: the initial glance literal)

## Background

The AYDY Companion extension (`extension/`, TypeScript, tested with vitest from the repo root) reads the hook's status files under `~/Library/Application Support/app.aydy/` and derives each window's glance itself, by the same rule as the Rust app, so it works with the app closed (`extension/src/sessionState.ts`: `parseStatus` `:52-85`, `isWorking` `:147-149`, `windowGlance` `:156-162`, `tabTitle` `:164-173`; reads in `extension/src/sessionStore.ts`). This task mirrors the Rust signals added in tasks 01_01–01_04 (`src-tauri/src/session/signals.rs`, `src-tauri/src/discovery/transcript.rs` `last_activity`). There is no TodoWrite checklist fallback (goals decision 10): the extension reads the transcript's and subagents' mtimes only, never transcript content.

The shared rule (spec `## Design`) — implement it exactly:

- `STALE_AFTER_MS = 5 * 60 * 1000`.
- Activity = max of: transcript mtime; mtime of every `*.jsonl` under `<dirname(transcript)>/<sessionId>/subagents/` (recursive, depth ≤ 3); the status file's mtime (`eventAt`); the accepted progress report's mtime. Transcript unreadable/absent → quiet unknown → never stale.
- Stale = working (`isWorking`) and now − activity ≥ `STALE_AFTER_MS`.
- Turn start = when the open turn began, the same state machine as Rust's `SessionTiming` (`src-tauri/src/session/timing.rs:40-54`, driven by `apply_hook` in `src-tauri/src/session/session.rs:111-144`). The status file holds only the latest event, so the extension carries the value across passes with a pure step function `nextTurnStart(previous: number | null, status: SessionStatus): number | null`:
  - `isWorking(status)` (`UserPromptSubmit`, or `Stop` with subagents running) → `previous ?? status.eventAt` (a turn already open keeps its start — Rust's `start_turn`);
  - `Stop` with no subagents running → `null` (Rust's `end_turn`);
  - anything else (`Notification`, `SessionStart`) → `previous`.
    It is idempotent, so applying it on every pass to an unchanged status changes nothing. Example the old rule got wrong: prompt at 10:00, report at 10:02, Stop with subagents running at 10:05 → turn start stays 10:00 and the 10:02 report is accepted; a report from 09:55 (the previous turn) is rejected, as in Rust. With no previous value (extension just started mid-turn) a Stop-with-subagents status starts the turn at its own `eventAt`, which is also what Rust does when it first sees that event outside an open turn.
- Progress report accepted iff `reportedAt >= turnStartedAt` (unknown turn start → accepted).
- Step = accepted report's step, else none.
- ETA remaining = `etaMs − (now − reportedAt)`, negative = overdue.
- Progress file: `app.aydy/progress/<id>.json`, body `{"etaSeconds":N,"step":{"done":D,"total":T}}` (`step` optional), mtime = report time; skip invalid id stems, bad JSON, missing/negative/non-integer `etaSeconds`; drop an invalid step (`total < 1`, `done > total`, non-integers) but keep the report; cap eta at 24 h.

Title format (decisions 4/5; the approved mock is not part of the repository, so its titles are inlined). Glyph, one space, signal text, one space, then the existing label (`aydy · feat/stale-and-progress-signals`):

| State                                | Title                                                |
| ------------------------------------ | ---------------------------------------------------- |
| Working, nothing to say (today)      | `◐ aydy · feat/stale-and-progress-signals`           |
| Agent step and ETA                   | `◐ 4/7 · ~6m aydy · feat/stale-and-progress-signals` |
| ETA only                             | `◐ ~6m aydy · …`                                     |
| Nothing reported (elapsed turn time) | `◐ 3m aydy · …`                                      |
| Past the ETA                         | `◐ 2m over aydy · …`                                 |
| Stale (wins over progress)           | `◌ quiet 9m aydy · feat/stale-and-progress-signals`  |
| Unread (unchanged, wins over both)   | `● 1 okven · main`                                   |

`◌` is U+25CC DOTTED CIRCLE; `◐` and `●` are the existing glyphs. Title durations are whole minutes so the workspace file is not rewritten every poll: `< 60 s` → `<1m`, `< 1 h` → `${floor(min)}m`, else `${h}h ${m}m`. Nothing to say (no step, eta or known turn start, not stale) → no signal, plain `◐`.

Most urgent in a window (decision 5): stale first (longest quiet wins), then the shortest ETA remaining (overdue counts as shortest), then the longest elapsed; ties keep input order. (Every report carries an ETA, so a step never needs its own tier.)

Patterns: pure exported arrow functions with tests beside them (`sessionState.test.ts`), defensive parsing of `unknown` (`parseStatus`, `field`, `parseJson` in `sessionStore.ts`), `listJson`/`idOf`/`errorCode` helpers and `pruneDir` (`sessionStore.ts:21-55, :181-215`), path helpers in `aydyPaths.ts`.

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `aydyPaths.ts`: `export const progressDir = (options = {}) => join(aydyDataDir(options), 'progress')`.
2. `sessionState.ts`:
   - `SessionStatus` gains `transcriptPath: string | null` (from `transcript_path` via `nonEmptyString`).
   - `export interface TitleSignal { stale: boolean; text: string }`; `WindowGlance` gains `signal: TitleSignal | null`; `windowGlance(statuses, seen, signal: TitleSignal | null = null)`.
   - `tabTitle`: unread branches unchanged; else if `glance.signal` → `${signal.stale ? '◌' : '◐'} ${signal.text} ${label}`; else today's `◐`/`○`.
     (This file is allowed more than one export — it already has many; keep the existing style.)
3. `workSignal.ts`:
   - `STALE_AFTER_MS`, `nextTurnStart(previous, status)`, `interface Step { done: number; total: number }`, `interface ProgressReport { etaMs: number; step: Step | null; reportedAt: number }`, `parseProgress(text: string, mtimeMs: number): ProgressReport | null`.
   - `interface SessionActivity { status: SessionStatus; turnStartedAt: number | null; transcriptWrittenAt: number | null; progress: ProgressReport | null }` — `turnStartedAt` is the caller's running `nextTurnStart` value (task 03_03 keeps it per session).
   - `interface SessionSignal { stale: boolean; quietMs: number | null; elapsedMs: number | null; step: Step | null; etaRemainingMs: number | null }`; `sessionSignal(activity, now): SessionSignal | null` (null unless `isWorking(status)`).
   - `mostUrgent(signals: readonly SessionSignal[]): SessionSignal | null`.
   - `titleSignal(signal: SessionSignal): TitleSignal | null` and `formatMinutes(ms)`.
4. `sessionStore.ts`:
   - `StoreDirs` gains `progress?: string`.
   - `readProgress({ progress = progressDir() } = {}): Promise<Map<string, ProgressReport>>` (read + stat each file, `parseProgress`).
   - `transcriptActivity(transcriptPath: string, sessionId: string): Promise<number | null>` — the newest mtime (ms) of the transcript and every `.jsonl` under `<dirname(transcript)>/<sessionId>/subagents/` (recursive, depth ≤ 3); missing transcript → `null`; an unreadable subagents dir is ignored. Stats only, no content read, no cache.
   - `pruneStale` also prunes the progress dir with the same 7-day rule.

## Gotchas

- The Rust and TS rules must agree; keep the wording of both in step and reuse the same test cases as `src-tauri/src/session/signals.rs` and `transcript.rs` (`last_activity`) tests where they apply.
- `parseStatus` callers and test fixtures build `SessionStatus` objects; add `transcriptPath: null` where they do (test edits).
- Do not wire anything into `windowTracker.ts` or `extension.ts` here beyond what compiles (task 03_03 does the wiring). `WindowGlance.signal` is required, so the one literal that builds a glance, `let glance: WindowGlance = { unread: 0, working: false }` at `extension.ts:88`, gains `signal: null`; `windowGlance`'s new parameter defaults to `null`, so `windowTracker.ts` compiles unchanged and titles stay as today until 03_03.
- `transcriptActivity` uses `stat`/`readdir` only; never read a transcript's content (it can be many megabytes, and nothing in it is needed).

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests:
  - `workSignal.test.ts`: `nextTurnStart` (prompt sets it; a second prompt or a Stop-with-subagents keeps it; Stop without subagents clears it; Notification keeps it; idempotent on repeat; Stop-with-subagents from `null` starts at its `eventAt`); a report written after the prompt but before a Stop-with-subagents is accepted and one from before the prompt rejected (the Rust 01_01 session test, mirrored); every bullet of the shared rule (stale at exactly 5 min, not at 4:59; unknown transcript never stale; status event newer than the transcript resets quiet; subagent activity counted via `transcriptWrittenAt` input; report before the turn start ignored; accepted report's step is the step, none without a report; ETA countdown and overdue); `parseProgress` validation and 24 h cap; `mostUrgent` ordering (stale beats ETA, longest quiet among stale, shortest ETA with overdue first, then elapsed); `titleSignal` texts `4/7 · ~6m`, `~6m`, `3m`, `2m over`, `quiet 9m`, `<1m`, `1h 10m`, and `null` when there is nothing to say.
  - `sessionState.test.ts`: `parseStatus` reads `transcript_path`; `tabTitle` renders `◐ 4/7 · ~6m label`, `◌ quiet 9m label`, unread still wins over a signal.
  - `sessionStore.test.ts`: `readProgress` reads valid files and skips invalid ones; `transcriptActivity` picks the newest of transcript and nested subagent `.jsonl` mtimes, ignores non-`.jsonl` files and files deeper than depth 3, returns `null` for a missing transcript; `pruneStale` prunes old progress files of dead sessions.
