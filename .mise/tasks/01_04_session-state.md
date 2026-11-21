# 01_04 Session state: parse, liveness, unread count, title text

## Goal

Add the extension modules that read the status and seen files, tell which sessions are alive, decide which are unread for a window, and format the tab title text.

## Files to modify/create

- extension/src/sessionState.ts (new, pure)
- extension/src/sessionStore.ts (new, IO)

## Background

- Task 01_01 produced `extension/src/aydyPaths.ts` (`statusDir()`, `seenDir()` and `claudeSessionsDir()`) and the hook script.
- Each `status/<session_id>.json` holds the raw Claude Code hook payload. Its **mtime is the event time**.
- `seen/<session_id>.json` holds `{"seenAt": <epoch ms>}`.
- Status mapping (as in `src-tauri/src/listener/routes.rs:61-86`):
  - `UserPromptSubmit` means working.
  - `Notification` means needsInput (any `notification_type`).
  - `Stop` means done.
  - `SessionStart` means done but never unread.
  - Anything else gives `null`.
- Unread rule: a session is unread when its last event is `Stop` or `Notification` and `eventAt > seenAt` (a missing seen file counts as 0). A session belongs to a window when its `cwd` equals or is inside one of the window's folders.
- Liveness comes from `~/.claude/sessions/<pid>.json` files with `pid` and `sessionId` fields, checked against the process table. The real shape is in `src-tauri/src/discovery/mod.rs:337-350`, and the app's equivalent is in `src-tauri/src/discovery/session_files.rs:1-35`.
- Title format (mock `.mise/mock/vscode-tab-count.html`):
  - count 0 gives `○ <name>`
  - count 1 gives `● <name>`
  - counts 2 to 9 give `● <n> <name>`
  - 10 or more gives `● 9+ <name>`

## Guides

None match.

## Implementation details

`sessionState.ts`:

- `type SessionEvent = 'SessionStart' | 'UserPromptSubmit' | 'Notification' | 'Stop'`.
- `interface SessionStatus { sessionId: string; event: SessionEvent; cwd: string | null; eventAt: number }`.
- `parseStatus(text, mtimeMs): SessionStatus | null` rejects bad JSON and missing or blank ids.
- `isUnread(status, seenAt: number | undefined)`.
- `ownsSession(cwd, folders: string[])` compares on path segments: `/a/b` owns `/a/b` and `/a/b/c`, not `/a/bc`. It strips trailing separators.
- `unreadCount(statuses, seen: Map<string, number>, live: Set<string>, folders)` counts only live sessions.
- `tabTitle(count, name)`.

`sessionStore.ts`. Every function takes an optional dirs object for tests:

- `readStatuses()` reads `*.json` in `statusDir`, skips dotfiles (temp files), and uses `stat.mtimeMs`. A missing dir returns `[]`.
- `readSeen()` returns a `Map`.
- `writeSeen(sessionId, ms)` uses `writeAtomically` from `extension/src/atomicWrite.ts` (task 01_02) after `mkdir -p`.
- `readLiveSessionIds(): Promise<Set<string> | null>` returns `null` when the sessions dir is unreadable. A pid is alive when `process.kill(pid, 0)` does not throw, or throws `EPERM`.
- `pruneStale(live, now)` does nothing when `live` is null. Otherwise it deletes status and seen files whose mtime is older than 7 days and whose session is not in `live`.

## Gotchas

- The session id becomes a file name. Accept only `^[A-Za-z0-9_-]{1,128}$`, matching the script.
- `fs.realpath` the folders once in the caller (task 01_05), not here. Keep `ownsSession` pure.
- `process.kill(pid, 0)` on pid 0 or a negative pid signals a process group. Reject pids that are not positive integers.

## Verification

- New `extension/src/sessionState.test.ts` covers:
  - `parseStatus` for each event, plus unknown event, bad JSON and bad id
  - `isUnread` at equal, older and newer seen times, and for working and SessionStart
  - `ownsSession` prefix edge cases
  - `unreadCount` excluding dead and foreign sessions
  - `tabTitle` at 0, 1, 2, 9, 10 and 12
- New `extension/src/sessionStore.test.ts` runs against temp dirs and covers:
  - read and write round-trip
  - temp files ignored
  - live detection using `process.pid` (alive) and a pid that does not exist
  - `null` for a missing sessions dir
  - prune removes only old dead files and does nothing when live is null
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
