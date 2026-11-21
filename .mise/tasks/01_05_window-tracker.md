# 01_05 Window tracker: mark seen on focus, compute the count

## Goal

Each VS Code window watches the status and seen files, marks its own sessions seen while it has focus, and knows its current unread count. This task wires the count up to a callback. Task 01_07 turns it into the tab title.

## Files to modify/create

- extension/src/windowTracker.ts (new)
- extension/src/extension.ts

## Background

- Task 01_04 produced:
  - In `extension/src/sessionState.ts`: `parseStatus`, `isUnread`, `ownsSession`, `unreadCount` and `tabTitle`.
  - In `extension/src/sessionStore.ts`: `readStatuses`, `readSeen`, `writeSeen`, `readLiveSessionIds` and `pruneStale`.
- Task 01_01 produced `extension/src/aydyPaths.ts` with `statusDir()` and `seenDir()`.
- Owner decision: a focused window marks every session it owns seen, both on gaining focus and whenever one of them changes while it is focused. So arriving clears the count, and a session that finishes while the window has focus is never counted.
- Debounce pattern: `extension/src/focusReporter.ts:27` (150 ms) and the timer handling at `:894-906`, which uses `unref()` on timers.
- Focus and folders in `extension.ts`:
  - `vscode.window.state.focused` (`extension.ts:116-119`)
  - `vscode.window.onDidChangeWindowState` (`:167`)
  - `vscode.workspace.workspaceFolders` (`:62`)
  - `onDidChangeWorkspaceFolders` (`:161`)
- Keep the existing `AydyClient` and `FocusReporter` wiring. It is removed in 03_08.

## Guides

None match.

## Implementation details

- `WindowTracker` options:
  - `folders: () => string[]` (already realpath'd)
  - `focused: () => boolean`
  - `onCount: (count: number) => void`
  - an optional `store` for tests (defaults to the `sessionStore` functions)
  - `debounceMs` (default 150) and `pollMs` (default 5000)
- Methods:
  - `start()`: `mkdir -p` both dirs, `fs.watch` each (ignoring watch errors, since polling covers them), start the poll interval (unref'd), run `pruneStale` once, and schedule a pass.
  - `poke()`: schedules a debounced pass.
  - `dispose()`.
- A pass:
  1. Read the statuses, seen times and live ids. Treat a `null` live set as "every session with a status is live", so tracking still works when `~/.claude/sessions` is unreadable.
  2. Take the owned live statuses.
  3. If `focused()`, write `seenAt = Date.now()` for each owned status where `isUnread(...)`, and update the in-memory seen map.
  4. Compute `unreadCount` and call `onCount` only when it differs from the last value. The first pass always calls it.
  5. Passes never overlap. A poke during a pass schedules one more pass.
- `extension.ts`:
  - Build the folder list by running `fs.realpathSync` on each workspace folder's `fsPath`, falling back to the raw path on error. Rebuild it on `onDidChangeWorkspaceFolders`.
  - Create the tracker and call `poke()` on `onDidChangeWindowState` and on folder changes.
  - For now, `onCount` stores the count in a module variable. Task 01_07 consumes it.
  - Dispose through `context.subscriptions`.

## Gotchas

- `fs.watch` on macOS can fire for the tracker's own seen writes. The debounce plus "write only when unread" keeps that from looping. Add a test that a focused pass followed by the watch event does not write again.
- A window with no folders owns nothing. Its count is 0 and it never writes seen files.
- Never throw out of a pass. Log nothing noisy, and swallow IO errors until the next pass.

## Verification

- New `extension/src/windowTracker.test.ts` uses fake timers and an injected store and covers:
  - an unfocused window reports the count and writes nothing
  - a focused window writes seen for owned unread sessions only and reports 0
  - a Stop arriving while focused stays at 0
  - focus loss followed by a new Stop gives 1
  - dead sessions are not counted
  - `onCount` is not repeated for an unchanged count
  - there are no overlapping passes
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
