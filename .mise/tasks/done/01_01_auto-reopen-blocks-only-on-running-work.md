# 01_01 Auto-reopen blocks only on running work

## Goal

The VS Code extension's "is it safe to auto-reopen this window as its workspace file"
check blocks only on things that are actually running and would be lost:

- a Claude Code session in this window whose last hook event is `UserPromptSubmit`
  (working) or `Notification` (waiting on the user);
- a terminal whose shell has a child process (dev server, build, Claude Code CLI);
- unsaved files (unchanged).

Idle sessions, Claude Code tabs on their own, and idle shells at a prompt no longer
block. Anything AYDY cannot determine blocks. README and setting copy describe the new
rule.

## Files to modify/create

- `extension/src/processTree.ts`
- `extension/src/autoReopen.ts`
- `extension/src/extension.ts`
- `extension/package.json`
- `extension/README.md`

(Tests: `extension/src/processTree.test.ts`, `extension/src/autoReopen.test.ts`.)

## Background

The branch already has the auto-reopen feature (commit `6485a74`). Today:

- `WindowContents` / `unsafeReasons` in `extension/src/autoReopen.ts:10-31` block on any
  terminal, any Claude tab, any live session under the window, or dirty docs.
  `listReasons` (`autoReopen.ts:33-37`) joins reasons as prose; keep it.
- `reasonsNotToReopen` in `extension/src/extension.ts:189-203` builds the input from
  `vscode.window.terminals.length`, the `isClaudeTab` tab count (`extension.ts:67`),
  `sessionsUnder(process.pid)` (failure counted as 1 session), and dirty
  `textDocuments`. It is used by `autoReopen` (`extension.ts:218-239`, which shows the
  notification) and by `migrateLegacy` (`extension.ts:247-271`, which silently skips
  when unsafe). Both callers keep their behavior; only the reasons change.
- `extension/src/processTree.ts`: `parseParents` (child → parent map from
  `ps -axo pid=,ppid=`), `descendsFrom`, private `readParents`, `readSessionPids` (reads
  `~/.claude/sessions/*.json`, keeps live `pid`s but drops `sessionId`), `sessionsUnder`.
  Session files look like `{"pid":19779,"sessionId":"0c0f…","cwd":"…",…}`;
  `extension/src/sessionStore.ts:150-165` reads the same files and validates
  `sessionId` with `SESSION_ID_PATTERN` from `extension/src/sessionState.ts`.
- Session state: `readStatuses()` in `extension/src/sessionStore.ts:60` returns
  `SessionStatus[]` (`sessionId`, `event`, …; `extension/src/sessionState.ts:1-9`). Only
  the last hook event per session is kept. `UserPromptSubmit` = working,
  `Notification` = waiting, `Stop`/`SessionStart` = idle.
- Test patterns: `unsafeReasons` cases in `extension/src/autoReopen.test.ts:38-65`;
  `parseParents`/`descendsFrom`/`readSessionPids` tests in
  `extension/src/processTree.test.ts` (vitest, temp dirs via `mkdtempSync`).

Owner decisions that reach this task:

- Sessions belong to this window by process ancestry: live sessions whose pid descends
  from this window's extension host (`process.pid`). A Claude Code CLI in an integrated
  terminal is not a descendant of the extension host (terminals are spawned by VS
  Code's pty host); the terminal check covers it.
- A Claude Code tab no longer blocks on its own.
- When in doubt, block: a terminal with no `processId` (pseudoterminal, or not known
  yet), a `Notification` event (it may be Claude Code's idle "waiting for your input"
  reminder, but it counts as waiting), or any failed check.
- A session with no status file yet is idle.
- A terminal is busy when its shell pid is the parent of any process in `ps`.
- Auto-reopen stays on by default, silent when safe, notification with **Not now** /
  **Reopen anyway** when not.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

`processTree.ts`

- Export `readParents` (unchanged body).
- Replace `readSessionPids` with `readLiveSessions(dir = claudeSessionsDir()):
Promise<{ pid: number; sessionId: string }[]>` — same reading/skipping rules, plus
  skip entries whose `sessionId` is not a string matching `SESSION_ID_PATTERN`.
- Replace `sessionsUnder` with a pure
  `sessionIdsUnder(sessions, root, parents): string[]` (filter by `descendsFrom`).
- Add a pure `hasChildren(pid: number, parents: ReadonlyMap<number, number>): boolean`
  — true when some entry's value equals `pid`.
- Remove `sessionsUnder` and `readSessionPids` once nothing uses them.

`autoReopen.ts`

- New `WindowContents`:
  `{ activeSessions: number; busyTerminals: number; dirtyDocuments: number; checkFailed: boolean }`.
- Add a pure `isActive(event: SessionEvent | undefined): boolean` — true for
  `UserPromptSubmit` and `Notification`, false for `Stop`, `SessionStart`, `undefined`.
  Optionally a pure `countActive(sessionIds, statuses: readonly SessionStatus[])` that
  joins by `sessionId` and counts `isActive`.
- `unsafeReasons` wording, in this order:
  - sessions: 1 → `a Claude Code session that is working or waiting on you`;
    more → `Claude Code sessions that are working or waiting on you`
  - terminals: 1 → `a terminal running a process`; more → `terminals running processes`
  - dirty: `unsaved files`
  - `checkFailed` → `processes AYDY could not check`

`extension.ts`

- Rewrite `reasonsNotToReopen`:
  1. `const parents = await readParents()`.
  2. Terminals: for each `vscode.window.terminals`, await `terminal.processId` raced
     against a short timeout (about 1 s) resolving `undefined`. Busy when the pid is
     `undefined` or `hasChildren(pid, parents)`.
  3. Sessions: `sessionIdsUnder(await readLiveSessions(), process.pid, parents)`, join
     with `await readStatuses()`; count active ones.
  4. Dirty docs as now.
  5. Wrap steps 1-3 so any thrown error is logged with the existing `log` and yields
     `checkFailed: true` (terminal and session counts 0 in that case, dirty docs still
     counted).
- Drop the `claudeTabs` input; keep `isClaudeTab` (still used at `extension.ts:354` and
  `:371`). Update imports (`readParents`, `readLiveSessions`, `sessionIdsUnder`,
  `hasChildren` from `./processTree`; `readStatuses` from `./sessionStore`).
- Notification text (`extension.ts:232`): the current "would end the ${list} open in
  it" reads badly with the new reasons. Use: `AYDY shows how many Claude Code sessions
  need you in this window’s tab by reopening it as a workspace. It did not reopen it
  automatically because this window has ${listReasons(reasons)}.`

`package.json` — `aydy.autoReopenAsWorkspace` `markdownDescription`: replace the last
sentence with one saying a window is never reopened without asking while a Claude Code
session in it is working or waiting on you, a terminal in it is running a process, or it
has unsaved files.

`README.md` — the "**Automatically.**" bullet (around line 103): same rule in prose;
mention that idle terminals and idle Claude Code sessions do not block, and that a
restarted session comes back idle.

## Gotchas

- `Terminal.processId` is a `Thenable<number | undefined>`; for a restored terminal it
  can stay pending, so it must be raced with a timeout or the check hangs.
- Some shell setups keep a background child (e.g. powerlevel10k's `gitstatusd`), so an
  idle shell can read busy. That errs on the blocking side; accept it, do not try to
  filter process names.
- The `ps` call is spawned by the extension host, not by a shell, so it never makes a
  shell look busy.
- `readStatuses` throws on read errors other than ENOENT; that must land in
  `checkFailed`, not escape.
- Keep `migrateLegacy` silent when unsafe; it only needs `reasons.length > 0`.
- Do not change `listReasons`, workspace-file logic, sweep, or migration paths.

## Verification

Run `yarn build-tsc`, `yarn lint`, `yarn test` from the repo root (Rust untouched;
`cargo clippy`/`cargo test` from `src-tauri/` should stay green).

Tests this task writes/updates:

- `extension/src/processTree.test.ts`:
  - `hasChildren`: true when a pid is some entry's parent, false for a leaf and for an
    unknown pid.
  - `sessionIdsUnder`: keeps sessions descending from the root, drops others.
  - Rework the `readSessionPids` test into `readLiveSessions`: returns
    `{ pid, sessionId }` for live entries; skips dead pids, broken JSON, non-`.json`
    files, and entries with a missing or invalid `sessionId`; missing dir → `[]`.
- `extension/src/autoReopen.test.ts`:
  - Replace the `unsafeReasons` cases for the new `WindowContents`: empty → `[]`; each
    field alone with singular and plural wording; all together in the documented order;
    `checkFailed` alone → `['processes AYDY could not check']`.
  - `isActive`: `UserPromptSubmit` and `Notification` true; `Stop`, `SessionStart`,
    `undefined` false. If `countActive` is added: a session with no status counts as
    idle, statuses for other session ids are ignored.

The VS Code glue in `reasonsNotToReopen` (terminal pid timeout, error → `checkFailed`)
has no unit test; verify by reading it against the steps above and by `yarn build-tsc`.
