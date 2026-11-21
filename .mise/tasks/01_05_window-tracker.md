# 01_05 Window tracker: publish folders, mark seen on focus, compute the count

## Goal

Each VS Code window publishes its folders, watches the status, seen and window files, marks the sessions it owns seen while it has focus, and knows its current unread count. A session is owned by exactly one window, so a focused parent window never clears a worktree window's unread sessions. This task wires the count up to a callback. Task 01_07 turns it into the tab title.

## Files to modify/create

- extension/src/windowTracker.ts (new)
- extension/src/extension.ts

## Background

- Task 01_04 produced:
  - In `extension/src/sessionState.ts`: `parseStatus`, `isUnread`, `containingFolderLength`, `ownerOf`, `unreadCount(statuses, seen, live, owns)`, `tabTitle` and the `WindowFolders` type.
  - In `extension/src/sessionStore.ts`: `readStatuses`, `readSeen`, `writeSeen`, `isAlive`, `readLiveSessionIds` and `pruneStale`.
  - In `extension/src/windowRegistry.ts`: `publishWindow(pid, folders)`, `unpublishWindow(pid)`, `readWindows()` and `knownWindows(selfPid, selfFolders)`.
- Task 01_01 produced `extension/src/aydyPaths.ts` with `statusDir()`, `seenDir()` and `windowsDir()`.
- Owner decision: a focused window marks every session it owns seen, both on gaining focus and whenever one of them changes while it is focused. So arriving clears the count, and a session that finishes while the window has focus is never counted.
- Ownership rule (`.mise/spec.md`, "What"): a session's owner is the open window with the longest folder containing its `cwd`, ties to the lowest pid (`ownerOf`). The tab count and the seen writes both use it. A plain prefix match is wrong: a repo window `/a` would count and clear sessions of a worktree window under `/a/.claude/worktrees/`.
- Debounce pattern: `extension/src/focusReporter.ts:27` (150 ms) and the timer handling around `:104`, which calls `unref()` on its timer.
- Focus and folders in `extension.ts`:
  - `vscode.window.state.focused` (`extension.ts:109`)
  - `vscode.window.onDidChangeWindowState` (`:159`)
  - `vscode.workspace.workspaceFolders` (`:54`)
  - `onDidChangeWorkspaceFolders` (`:153`)
- The window's identity is `process.pid` of its extension host, as `windowFacts` already uses (`extension.ts:52-58`).
- Keep the existing `AydyClient` and `FocusReporter` wiring. It is removed in 03_08.

## Guides

None match.

## Implementation details

- `WindowTracker` options:
  - `pid: number` (this window's `process.pid`)
  - `folders: () => string[]` (already realpath'd)
  - `focused: () => boolean`
  - `onCount: (count: number) => void`
  - an optional `store` for tests (defaults to the `sessionStore` functions plus `knownWindows`)
  - `debounceMs` (default 150) and `pollMs` (default 5000)
- Methods:
  - `start()`: `mkdir -p` the status, seen and windows dirs, `fs.watch` each (ignoring watch errors, since polling covers them), start the poll interval (unref'd), run `pruneStale` once, and schedule a pass.
  - `poke()`: schedules a debounced pass.
  - `dispose()`.
- A pass:
  1. Read the statuses, seen times, live ids and `knownWindows(pid, folders())`. Treat a `null` live set as "every session with a status is live", so tracking still works when `~/.claude/sessions` is unreadable.
  2. Take the live statuses with `ownerOf(status.cwd, windows) === pid`.
  3. If `focused()`, write `seenAt = Date.now()` for each of those owned statuses where `isUnread(...)`, and update the in-memory seen map. Never write seen for a session another window owns.
  4. Compute `unreadCount` with `owns = (cwd) => ownerOf(cwd, windows) === pid` and call `onCount` only when it differs from the last value. The first pass always calls it.
  5. Passes never overlap. A poke during a pass schedules one more pass.
- `extension.ts`:
  - Build the folder list by running `fs.realpathSync` on each workspace folder's `fsPath`, falling back to the raw path on error. Rebuild it on `onDidChangeWorkspaceFolders`.
  - Publish: call `publishWindow(process.pid, folders)` on activation and after every folder rebuild. Push a disposable onto `context.subscriptions` that calls `unpublishWindow(process.pid)`, and call it from `deactivate` as well. Swallow errors from both.
  - Create the tracker with `pid: process.pid` and call `poke()` on `onDidChangeWindowState` and on folder changes.
  - For now, `onCount` stores the count in a module variable. Task 01_07 consumes it.
  - Dispose through `context.subscriptions`.

## Gotchas

- `fs.watch` on macOS can fire for the tracker's own seen writes. The debounce plus "write only when unread" keeps that from looping. Add a test that a focused pass followed by the watch event does not write again.
- Every window republishes on folder changes, and the windows dir watch pokes every tracker. That is a few passes per change, not a loop: publishing does not happen inside a pass.
- A window with no folders owns nothing. Its count is 0 and it never writes seen files. It still publishes (with `folders: []`), which is harmless.
- Never throw out of a pass. Log nothing noisy, and swallow IO errors until the next pass.

## Verification

- New `extension/src/windowTracker.test.ts` uses fake timers and an injected store and covers:
  - an unfocused window reports the count and writes nothing
  - a focused window writes seen for owned unread sessions only and reports 0
  - nested windows: with this window at `/a` and another known window (pid different) at `/a/.claude/worktrees/x`, a Stop with `cwd` in the worktree is not counted here, and a focused pass here writes no seen for it; with the roles swapped (this window is the worktree), it is counted and cleared on focus
  - a Stop arriving while focused stays at 0
  - focus loss followed by a new Stop gives 1
  - dead sessions are not counted
  - `onCount` is not repeated for an unchanged count
  - there are no overlapping passes
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
