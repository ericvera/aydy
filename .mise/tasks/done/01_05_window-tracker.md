# 01_05 Window tracker: publish folders, mark seen on focus, compute the count

## Goal

Each VS Code window publishes its folders, watches the status, seen and window files, marks the sessions it owns seen while it has focus, and knows its current unread count. A session is owned by exactly one window, so a focused parent window never clears a worktree window's unread sessions. This task wires the count up to a callback. Task 01_07 turns it into the tab title.

## Files to modify/create

- extension/src/windowTracker.ts (new)
- extension/src/extension.ts

## Background

- Task 01_04 produced:
  - In `extension/src/sessionState.ts`: `parseStatus`, `isUnread`, `sessionDir(status, launchCwds)`, `containingFolderLength`, `ownerOf`, `unreadCount(statuses, seen)` (counts unread among the statuses given), `tabTitle` and the `WindowFolders` type.
  - In `extension/src/sessionStore.ts`: `readStatuses`, `readSeen`, `writeSeen`, `isAlive`, `readLiveSessions` (live session id to launch `cwd`, or `null` when `~/.claude/sessions` is unreadable), `resolveCwd` (realpath, raw value on failure) and `pruneStale`. Each takes an optional dirs object.
  - In `extension/src/windowRegistry.ts`: `publishWindow(pid, folders)`, `unpublishWindow(pid)`, `readWindows()` and `knownWindows(selfPid, selfFolders)`.
- Task 01_01 produced `extension/src/aydyPaths.ts` with `statusDir()`, `seenDir()`, `windowsDir()` and `claudeSessionsDir()`.
- Owner decision: a focused window marks every session it owns seen, both on gaining focus and whenever one of them changes while it is focused. So arriving clears the count, and a session that finishes while the window has focus is never counted.
- Ownership rule (`.mise/spec.md`, "What"): a session's owner is the open window with the longest folder containing its session directory, ties to the lowest pid (`ownerOf`). The tab count and the seen writes both use it. A plain prefix match is wrong: a repo window `/a` would count and clear sessions of a worktree window under `/a/.claude/worktrees/`.
- Session directory (`.mise/spec.md`, "Session directory"): `resolveCwd(sessionDir(status, live))`, the launch directory from the live session file, else the payload `cwd`, realpath'd. The payload `cwd` moves when a session changes directory, so it is only the fallback.
- Remote windows are out of scope (spec, "What"). Only `file`-scheme workspace folders count as this window's folders, so a remote window owns nothing.
- Verification boundary (spec): no test may create, watch or write anything under the real `~/Library/Application Support/app.aydy/` or read the real `~/.claude/sessions`.
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
  - `dirs?: {status: string; seen: string; windows: string; sessions: string}`, defaulting to `statusDir()`, `seenDir()`, `windowsDir()` and `claudeSessionsDir()`. Every directory the tracker creates or watches, and every default store function, uses these.
  - an optional `store` for tests (defaults to the `sessionStore` functions and `knownWindows`, bound to `dirs`): `readStatuses`, `readSeen`, `writeSeen`, `readLiveSessions`, `resolveCwd`, `pruneStale` and `knownWindows`
  - `debounceMs` (default 150) and `pollMs` (default 5000)
- Methods:
  - `start()`: `mkdir -p` `dirs.status`, `dirs.seen` and `dirs.windows`, `fs.watch` each (ignoring watch errors, since polling covers them), start the poll interval (unref'd), run `pruneStale(await readLiveSessions(), Date.now())` once (both through `store`, bound to `dirs`; `pruneStale(null, ...)` is a no-op per 01_04, so an unreadable `~/.claude/sessions` deletes nothing), and schedule a pass.
  - `poke()`: schedules a debounced pass.
  - `dispose()`.
- A pass:
  1. Read the statuses, seen times, live sessions (`readLiveSessions`) and `knownWindows(pid, folders())`. Treat a `null` live map as "every session with a status is live, with no launch directory known", so tracking still works when `~/.claude/sessions` is unreadable.
  2. For each live status, compute its directory `await resolveCwd(sessionDir(status, live))`, and keep those with `ownerOf(directory, windows) === pid`.
  3. If `focused()`, write `seenAt = Date.now()` for each of those owned statuses where `isUnread(...)`, and update the in-memory seen map. Never write seen for a session another window owns.
  4. Compute `unreadCount(owned, seen)` and call `onCount` only when it differs from the last value. The first pass always calls it.
  5. Passes never overlap. A poke during a pass schedules one more pass.
- `extension.ts`:
  - Build the folder list from the workspace folders whose `uri.scheme === 'file'`, running `fs.realpathSync` on each `fsPath` and falling back to the raw path on error. Rebuild it on `onDidChangeWorkspaceFolders`.
  - Publish: call `publishWindow(process.pid, folders)` on activation and after every folder rebuild. Push a disposable onto `context.subscriptions` that calls `unpublishWindow(process.pid)`, and call it from `deactivate` as well. Swallow errors from both.
  - Create the tracker with `pid: process.pid` (default `dirs`) and call `poke()` on `onDidChangeWindowState` and on folder changes.
  - For now, `onCount` stores the count in a module variable. Task 01_07 consumes it.
  - Dispose through `context.subscriptions`.

## Gotchas

- `fs.watch` on macOS can fire for the tracker's own seen writes. The debounce plus "write only when unread" keeps that from looping. Add a test that a focused pass followed by the watch event does not write again.
- Every window republishes on folder changes, and the windows dir watch pokes every tracker. That is a few passes per change, not a loop: publishing does not happen inside a pass.
- A window with no folders owns nothing. Its count is 0 and it never writes seen files. It still publishes (with `folders: []`), which is harmless.
- Never throw out of a pass. Log nothing noisy, and swallow IO errors until the next pass.

## Verification

- New `extension/src/windowTracker.test.ts` uses fake timers, an injected store and `dirs` pointing into a fresh `fs.mkdtemp` directory for every tracker it starts. No test constructs a tracker without `dirs`. It covers:
  - an unfocused window reports the count and writes nothing
  - a focused window writes seen for owned unread sessions only and reports 0
  - nested windows: with this window at `/a` and another known window (pid different) at `/a/.claude/worktrees/x`, a Stop with `cwd` in the worktree is not counted here, and a focused pass here writes no seen for it; with the roles swapped (this window is the worktree), it is counted and cleared on focus
  - a Stop arriving while focused stays at 0
  - focus loss followed by a new Stop gives 1
  - dead sessions are not counted
  - moved session: windows `/a` (this one) and `/a/.claude/worktrees/x` (another pid); a live Stop whose payload `cwd` is in the worktree but whose launch `cwd` (from `readLiveSessions`) is `/a` is counted here and cleared by a focused pass here, and with this window as the worktree it is neither counted nor marked seen
  - fallback: with `readLiveSessions` returning `null`, or a map without the session, the payload `cwd` decides ownership
  - resolution: the store's `resolveCwd` is applied before `ownerOf` (a fake that maps `/link` to `/a` makes a session launched in `/link` owned by the `/a` window)
  - `start()` creates the three directories inside the temp `dirs`
  - `onCount` is not repeated for an unchanged count
  - there are no overlapping passes
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
