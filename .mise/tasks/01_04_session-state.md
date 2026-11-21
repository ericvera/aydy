# 01_04 Session state: parse, liveness, window ownership, unread count, title text

## Goal

Add the extension modules that read the status and seen files, tell which sessions are alive, publish and read the list of open VS Code windows, decide which single window owns each session, count a window's unread sessions, and format the tab title text.

## Files to modify/create

- extension/src/sessionState.ts (new, pure)
- extension/src/sessionStore.ts (new, IO)
- extension/src/windowRegistry.ts (new, IO)

## Background

- Task 01_01 produced `extension/src/aydyPaths.ts` (`statusDir()`, `seenDir()`, `windowsDir()` and `claudeSessionsDir()`) and the hook script. Task 01_02 produced `writeAtomically` in `extension/src/atomicWrite.ts`.
- Each `status/<session_id>.json` holds the raw Claude Code hook payload. Its **mtime is the event time**.
- `seen/<session_id>.json` holds `{"seenAt": <epoch ms>}`.
- `windows/<pid>.json` holds `{"pid": <number>, "folders": [<realpath>, ...]}`, one per open VS Code window, written by that window's extension host (`pid` is its `process.pid`). Task 01_05 publishes it.
- Status mapping (as in `src-tauri/src/listener/routes.rs:61-86`):
  - `UserPromptSubmit` means working.
  - `Notification` means needsInput (any `notification_type`).
  - `Stop` means done.
  - `SessionStart` means done but never unread.
  - Anything else gives `null`.
- Unread rule: a session is unread when its last event is `Stop` or `Notification` and `eventAt > seenAt` (a missing seen file counts as 0).
- Ownership rule (`.mise/spec.md`, "What"): a session belongs to **exactly one** window. Of all open windows with a folder that equals or contains the session's `cwd`, the owner is the window whose containing folder is the longest. Ties go to the lowest pid. Without this, a repo window `/a` and a worktree window `/a/.claude/worktrees/x` would both count, mark seen, and claim jumps for the worktree's sessions.
- Liveness comes from `~/.claude/sessions/<pid>.json` files with `pid` and `sessionId` fields, checked against the process table. The real shape is `REAL_SESSION_FILE` in `src-tauri/src/discovery/mod.rs:48`, and the app's equivalent is in `src-tauri/src/discovery/session_files.rs:1-35`.
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
- `interface WindowFolders { pid: number; folders: string[] }`.
- `parseStatus(text, mtimeMs): SessionStatus | null` rejects bad JSON and missing or blank ids.
- `isUnread(status, seenAt: number | undefined)`.
- `containingFolderLength(cwd: string | null, folders: string[]): number` compares on path segments: `/a/b` contains `/a/b` and `/a/b/c`, not `/a/bc`. It strips trailing separators and returns the length of the longest containing folder, or `-1` (also for a null cwd).
- `ownerOf(cwd, windows: WindowFolders[]): number | null` returns the pid with the largest `containingFolderLength`, ties to the lowest pid, or `null` when no window contains `cwd`.
- `unreadCount(statuses, seen: Map<string, number>, live: Set<string>, owns: (cwd: string | null) => boolean)` counts only live, owned, unread sessions.
- `tabTitle(count, name)`.

`sessionStore.ts`. Every function takes an optional dirs object for tests:

- `readStatuses()` reads `*.json` in `statusDir`, skips dotfiles (temp files), and uses `stat.mtimeMs`. A missing dir returns `[]`.
- `readSeen()` returns a `Map`.
- `writeSeen(sessionId, ms)` uses `writeAtomically` after `mkdir -p`.
- `isAlive(pid: unknown): boolean` is true when `pid` is a positive integer and `process.kill(pid, 0)` does not throw, or throws `EPERM`.
- `readLiveSessionIds(): Promise<Set<string> | null>` returns `null` when the sessions dir is unreadable, and otherwise the ids whose pid `isAlive`.
- `pruneStale(live, now)` does nothing when `live` is null. Otherwise it deletes status and seen files whose mtime is older than 7 days and whose session is not in `live`.

`windowRegistry.ts`. Every function takes an optional `{dir?: string, isAlive?: (pid: number) => boolean}` for tests, defaulting to `windowsDir()` and `isAlive` from `sessionStore.ts`:

- `publishWindow(pid, folders)` runs `mkdir -p`, then `writeAtomically(<dir>/<pid>.json, JSON.stringify({pid, folders}))`.
- `unpublishWindow(pid)` unlinks `<dir>/<pid>.json`, ignoring `ENOENT`.
- `readWindows(): Promise<WindowFolders[]>` reads every `<digits>.json`, skips dotfiles and files that fail to parse or whose `pid` differs from the file name or whose `folders` is not a string array. For an entry whose pid is not alive it unlinks the file (ignoring errors) and leaves it out. A missing dir returns `[]`.
- `knownWindows(selfPid, selfFolders): Promise<WindowFolders[]>` returns `readWindows()` without any entry for `selfPid`, plus `{pid: selfPid, folders: selfFolders}`. The caller's own folders always come from memory, never from its file.

## Gotchas

- The session id becomes a file name. Accept only `^[A-Za-z0-9_-]{1,128}$`, matching the script.
- `fs.realpath` the folders once in the caller (task 01_05), not here. Keep `sessionState.ts` pure.
- `process.kill(pid, 0)` on pid 0 or a negative pid signals a process group. `isAlive` rejects pids that are not positive integers.
- A window file can outlive a crashed window and its pid can later be reused by another process. Normal closes delete the file (01_05 calls `unpublishWindow` on dispose), so this only follows a crash. Accept it, and do not add heartbeats.
- Ties on folder length only happen when the same folder is open in two windows. The lowest-pid rule must be the same in every window, so never break ties by "self first".

## Verification

- New `extension/src/sessionState.test.ts` covers:
  - `parseStatus` for each event, plus unknown event, bad JSON and bad id
  - `isUnread` at equal, older and newer seen times, and for working and SessionStart
  - `containingFolderLength` prefix edge cases (`/a/bc`, trailing slash, equal path, null cwd)
  - `ownerOf`: a session under `/a/.claude/worktrees/x` is owned by the window holding that worktree, not by the `/a` window; a session directly in `/a` is owned by the `/a` window; a tie goes to the lower pid; no containing window gives `null`
  - `unreadCount` excluding dead, unowned and read sessions
  - `tabTitle` at 0, 1, 2, 9, 10 and 12
- New `extension/src/sessionStore.test.ts` runs against temp dirs and covers:
  - read and write round-trip
  - temp files ignored
  - `isAlive` for `process.pid` (alive), a pid that does not exist, `0`, `-1` and `1.5`
  - live detection using `process.pid` (alive) and a pid that does not exist
  - `null` for a missing sessions dir
  - prune removes only old dead files and does nothing when live is null
- New `extension/src/windowRegistry.test.ts` runs against a temp dir with an injected `isAlive` and covers:
  - publish then read round-trip
  - a dead pid's file is left out and deleted
  - malformed files and a pid that differs from the file name are skipped
  - `unpublishWindow` removes the file and ignores a missing one
  - `knownWindows` replaces a stale self entry with the in-memory folders
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
