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

The AYDY Companion extension (`extension/`, TypeScript, tested with vitest from the repo root) reads the hook's status files under `~/Library/Application Support/app.aydy/` and derives each window's glance itself, by the same rule as the Rust app, so it works with the app closed (`extension/src/sessionState.ts`: `parseStatus` `:52-85`, `isWorking` `:157-159`, `windowGlance` `:166-172`, `tabTitle` `:174-183`; reads in `extension/src/sessionStore.ts`). This task mirrors the Rust signals added in tasks 01_01–01_04 (`src-tauri/src/session/signals.rs`, `src-tauri/src/discovery/transcript.rs` checklist and `last_activity`).

The shared rule (spec `## Design`) — implement it exactly:

- `STALE_AFTER_MS = 5 * 60 * 1000`.
- Activity = max of: transcript mtime; mtime of every `*.jsonl` under `<dirname(transcript)>/<sessionId>/subagents/` (recursive, depth ≤ 3); the status file's mtime (`eventAt`); the accepted progress report's mtime. Transcript unreadable/absent → quiet unknown → never stale.
- Stale = working (`isWorking`) and now − activity ≥ `STALE_AFTER_MS`.
- Turn start (extension) = `eventAt` when the status event is `UserPromptSubmit`; otherwise unknown.
- Progress report accepted iff `reportedAt >= turnStart` (unknown turn start → accepted).
- Step = accepted report's step, else the transcript checklist.
- Checklist: newest non-sidechain assistant entry with a `tool_use` named `TodoWrite` (`message.content[]` element `{type:"tool_use", name:"TodoWrite", input:{todos:[{status}]}}`) after the newest genuine prompt; `done` = todos with `status === "completed"`, `total` = length; total 0 → none. Genuine prompt = `type: "user"`, not `isSidechain`, not `isMeta`, `message.content` a string or an array with no `type: "tool_result"` element. Scan the tail's lines newest-first; skip unparseable lines.
- ETA remaining = `etaMs − (now − reportedAt)`, negative = overdue.
- Progress file: `app.aydy/progress/<id>.json`, body `{"etaSeconds":N,"step":{"done":D,"total":T}}` (`step` optional), mtime = report time; skip invalid id stems, bad JSON, missing/negative/non-integer `etaSeconds`; drop an invalid step (`total < 1`, `done > total`, non-integers) but keep the report; cap eta at 24 h.

Title format (decision 4/5 and `.mise/mock/b-vscode-title.html`): `◐ 4/7 · ~6m aydy · branch`, `◐ 2/5 …`, `◐ ~6m …`, `◐ 3m …` (elapsed), `◌ quiet 9m …`; overdue `◐ 2m over …`. Title durations are whole minutes so the workspace file is not rewritten every poll: `< 60 s` → `<1m`, `< 1 h` → `${floor(min)}m`, else `${h}h ${m}m`. Nothing to say (no step, eta or known turn start, not stale) → no signal, plain `◐`.

Most urgent in a window (decision 5): stale first (longest quiet wins), then the shortest ETA remaining (overdue counts as shortest), then the highest step fraction, then the longest elapsed; ties keep input order.

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
   - `STALE_AFTER_MS`, `interface Step { done: number; total: number }`, `interface ProgressReport { etaMs: number; step: Step | null; reportedAt: number }`, `parseProgress(text: string, mtimeMs: number): ProgressReport | null`, `checklistFromTail(text: string): Step | null`.
   - `interface SessionActivity { status: SessionStatus; transcriptWrittenAt: number | null; checklist: Step | null; progress: ProgressReport | null }`.
   - `interface SessionSignal { stale: boolean; quietMs: number | null; elapsedMs: number | null; step: Step | null; etaRemainingMs: number | null }`; `sessionSignal(activity, now): SessionSignal | null` (null unless `isWorking(status)`).
   - `mostUrgent(signals: readonly SessionSignal[]): SessionSignal | null`.
   - `titleSignal(signal: SessionSignal): TitleSignal | null` and `formatMinutes(ms)`.
4. `sessionStore.ts`:
   - `StoreDirs` gains `progress?: string`.
   - `readProgress({ progress = progressDir() } = {}): Promise<Map<string, ProgressReport>>` (read + stat each file, `parseProgress`).
   - `transcriptActivity(transcriptPath: string, sessionId: string, cache: Map<string, TailCache>): Promise<{ writtenAt: number | null; checklist: Step | null }>` — stat the transcript (missing → `{ writtenAt: null, checklist: null }`), walk the subagents dir for the newest `.jsonl` mtime, and read the last 256 KiB (dropping a leading partial line when not at offset 0) for `checklistFromTail` only when the transcript's size or mtime differs from the cached entry.
   - `pruneStale` also prunes the progress dir with the same 7-day rule.

## Gotchas

- The Rust and TS rules must agree; keep the wording of both in step and reuse the same test cases as `src-tauri/src/session/signals.rs` and `transcript.rs` tests where they apply.
- `parseStatus` callers and test fixtures build `SessionStatus` objects; add `transcriptPath: null` where they do (test edits).
- Do not wire anything into `windowTracker.ts` or `extension.ts` here beyond what compiles (task 03_03 does the wiring). `WindowGlance.signal` is required, so the one literal that builds a glance, `let glance: WindowGlance = { unread: 0, working: false }` at `extension.ts:88`, gains `signal: null`; `windowGlance`'s new parameter defaults to `null`, so `windowTracker.ts` compiles unchanged and titles stay as today until 03_03.
- Tail reads use `open` + `read` at an offset (`node:fs/promises` `FileHandle`), not `readFile` of a multi-megabyte transcript.

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests:
  - `workSignal.test.ts`: every bullet of the shared rule (stale at exactly 5 min, not at 4:59; unknown transcript never stale; status event newer than the transcript resets quiet; subagent activity counted via `transcriptWrittenAt` input; report before the turn start ignored; agent step over checklist; ETA countdown and overdue); `checklistFromTail` cases (newest TodoWrite after the last genuine prompt; tool_result entries do not end the turn; `isMeta` ignored; sidechain ignored; before the prompt → null; empty todos → null; unparseable lines skipped); `parseProgress` validation and 24 h cap; `mostUrgent` ordering (stale beats ETA, longest quiet among stale, shortest ETA with overdue first, then fraction, then elapsed); `titleSignal` texts `4/7 · ~6m`, `2/5`, `~6m`, `3m`, `2m over`, `quiet 9m`, `<1m`, `1h 10m`, and `null` when there is nothing to say.
  - `sessionState.test.ts`: `parseStatus` reads `transcript_path`; `tabTitle` renders `◐ 4/7 · ~6m label`, `◌ quiet 9m label`, unread still wins over a signal.
  - `sessionStore.test.ts`: `readProgress` reads valid files and skips invalid ones; `transcriptActivity` picks the newest of transcript and nested subagent `.jsonl` mtimes, returns nulls for a missing transcript, reads the checklist, and does not re-read an unchanged transcript (assert via the cache); `pruneStale` prunes old progress files of dead sessions.
