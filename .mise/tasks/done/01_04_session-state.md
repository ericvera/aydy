# 01_04 Session state: parse, liveness, window ownership, unread count, title text

## Goal

Add the extension modules that read the status and seen files, tell which sessions are alive and which directory each was launched in, publish and read the list of open VS Code windows, decide which single window owns each session, count a window's unread sessions, and format the tab title text.

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
- Ownership rule (`.mise/spec.md`, "What"): a session belongs to **exactly one** window. Of all open windows with a folder that equals or contains the session's directory, the owner is the window whose containing folder is the longest. Ties go to the lowest pid. Without this, a repo window `/a` and a worktree window `/a/.claude/worktrees/x` would both count, mark seen, and claim jumps for the worktree's sessions.
- Session directory (`.mise/spec.md`, "Session directory"): the hook payload's `cwd` is Claude Code's current directory at hook time, so it moves when a session changes directory. A session in window `/a` that enters `.claude/worktrees/x` would otherwise become owned by the worktree window. Ownership keys on the **launch** directory: the `cwd` field of the session's live `~/.claude/sessions/<pid>.json`, falling back to the payload `cwd` only when no live session file for that id carries one. It is realpath'd (raw value on failure) before comparing against the realpath'd window folders.
- Liveness and the launch directory both come from `~/.claude/sessions/<pid>.json` files with `pid`, `sessionId` and `cwd` fields, checked against the process table. The real shape is `REAL_SESSION_FILE` in `src-tauri/src/discovery/mod.rs:48-60`, and the app's equivalent reader is in `src-tauri/src/discovery/session_files.rs:1-35`.
- Title format (mock `.mise/mock/vscode-tab-count.html`):
  - count 0 gives `○ <name>`
  - count 1 gives `● <name>`
  - counts 2 to 9 give `● <n> <name>`
  - 10 or more gives `● 9+ <name>`

## Guides

None match.

## Implementation details

Start with the session-file `cwd` check at the top of "Verification". It can stop this task.

`sessionState.ts`:

- `type SessionEvent = 'SessionStart' | 'UserPromptSubmit' | 'Notification' | 'Stop'`.
- `interface SessionStatus { sessionId: string; event: SessionEvent; cwd: string | null; eventAt: number }`.
- `interface WindowFolders { pid: number; folders: string[] }`.
- `parseStatus(text, mtimeMs): SessionStatus | null` rejects bad JSON and missing or blank ids. `cwd` is the payload's `cwd` (null when absent or not a non-empty string).
- `isUnread(status, seenAt: number | undefined)`.
- `sessionDir(status, launchCwds: ReadonlyMap<string, string | null> | null): string | null` returns `launchCwds?.get(status.sessionId) ?? status.cwd`: the launch directory when known, else the payload `cwd`. It does not resolve; the caller passes the result through `resolveCwd`.
- `containingFolderLength(cwd: string | null, folders: string[]): number` compares on path segments: `/a/b` contains `/a/b` and `/a/b/c`, not `/a/bc`. It strips trailing separators and returns the length of the longest containing folder, or `-1` (also for a null cwd).
- `ownerOf(cwd, windows: WindowFolders[]): number | null` returns the pid with the largest `containingFolderLength`, ties to the lowest pid, or `null` when no window contains `cwd`. Callers pass a resolved session directory, never a raw payload `cwd`.
- `unreadCount(statuses, seen: Map<string, number>)` counts the statuses for which `isUnread` holds. The caller (01_05) passes only live, owned statuses.
- `tabTitle(count, name)`.

`sessionStore.ts`. Every function takes an optional dirs object for tests:

- `readStatuses()` reads `*.json` in `statusDir`, skips dotfiles (temp files), and uses `stat.mtimeMs`. A missing dir returns `[]`.
- `readSeen()` returns a `Map`.
- `writeSeen(sessionId, ms)` uses `writeAtomically` after `mkdir -p`.
- `isAlive(pid: unknown): boolean` is true when `pid` is a positive integer and `process.kill(pid, 0)` does not throw, or throws `EPERM`.
- `readLiveSessions(): Promise<Map<string, string | null> | null>` returns `null` when the sessions dir is unreadable. Otherwise it reads every `*.json` there, skips files that fail to parse or lack a string `sessionId` or a `pid`, keeps those whose `pid` `isAlive`, and maps each live session id to its launch `cwd` (the file's `cwd` when a non-empty string, else `null`). When two live files share an id, a non-null `cwd` wins.
- `resolveCwd(cwd: string | null): Promise<string | null>` returns `null` for `null`, otherwise `fs.promises.realpath(cwd)`, or `cwd` unchanged when that throws (for example a deleted directory).
- `pruneStale(live: ReadonlyMap<string, unknown> | null, now)` does nothing when `live` is null. Otherwise it deletes status and seen files whose mtime is older than 7 days and whose session is not in `live`.

`windowRegistry.ts`. Every function takes an optional `{dir?: string, isAlive?: (pid: number) => boolean}` for tests, defaulting to `windowsDir()` and `isAlive` from `sessionStore.ts`:

- `publishWindow(pid, folders)` runs `mkdir -p`, then `writeAtomically(<dir>/<pid>.json, JSON.stringify({pid, folders}))`.
- `unpublishWindow(pid)` unlinks `<dir>/<pid>.json`, ignoring `ENOENT`.
- `readWindows(): Promise<WindowFolders[]>` reads every `<digits>.json`, skips dotfiles and files that fail to parse or whose `pid` differs from the file name or whose `folders` is not a string array. For an entry whose pid is not alive it unlinks the file (ignoring errors) and leaves it out. A missing dir returns `[]`.
- `knownWindows(selfPid, selfFolders): Promise<WindowFolders[]>` returns `readWindows()` without any entry for `selfPid`, plus `{pid: selfPid, folders: selfFolders}`. The caller's own folders always come from memory, never from its file.

## Gotchas

- The session id becomes a file name. Accept only `^[A-Za-z0-9_-]{1,128}$`, matching the script.
- `fs.realpath` the folders once in the caller (task 01_05), not here. Keep `sessionState.ts` pure: resolving a session directory is `resolveCwd` in `sessionStore.ts`.
- Never key ownership on the payload `cwd` when a live session file gives a launch `cwd`. The payload value is only the fallback.
- `process.kill(pid, 0)` on pid 0 or a negative pid signals a process group. `isAlive` rejects pids that are not positive integers.
- A window file can outlive a crashed window and its pid can later be reused by another process. Normal closes delete the file (01_05 calls `unpublishWindow` on dispose), so this only follows a crash. Accept it, and do not add heartbeats.
- Ties on folder length only happen when the same folder is open in two windows. The lowest-pid rule must be the same in every window, so never break ties by "self first".

## Verification

- Check first, before writing code, that the session file's `cwd` does not move. Read-only, touching nothing:
  1. For each `~/.claude/sessions/*.json` whose pid is alive, read its `cwd` and `sessionId`, then the transcript `~/.claude/projects/*/<sessionId>.jsonl`, whose lines carry the session's `cwd` at each turn.
  2. If some transcript's later lines show a `cwd` that differs from its first line's (the session changed directory), compare: the session file's `cwd` equal to the first line's confirms it stays fixed.
  3. If the session file's `cwd` equals a later, moved directory instead, **stop**. Write no code and report `Blocked: the session file cwd follows the session's current directory` with the file names and the values seen. The ownership rule then needs another source and goes back to the owner.
  4. If no live session ever changed directory, the check is inconclusive. Proceed, and state `Session-file cwd check: inconclusive` in your report so the owner verifies it at acceptance (spec, "Verification boundary"). Do not start a Claude session to provoke a change: its hooks would fire against the owner's real settings and app.

- New `extension/src/sessionState.test.ts` covers:
  - `parseStatus` for each event, plus unknown event, bad JSON and bad id
  - `isUnread` at equal, older and newer seen times, and for working and SessionStart
  - `containingFolderLength` prefix edge cases (`/a/bc`, trailing slash, equal path, null cwd)
  - `ownerOf`: a session under `/a/.claude/worktrees/x` is owned by the window holding that worktree, not by the `/a` window; a session directly in `/a` is owned by the `/a` window; a tie goes to the lower pid; no containing window gives `null`
  - `sessionDir`: a launch `cwd` of `/a` wins over a payload `cwd` of `/a/.claude/worktrees/x` (so with windows `/a` and the worktree, `ownerOf(sessionDir(...))` is the `/a` window); a missing map entry, a `null` entry and a `null` map all fall back to the payload `cwd`
  - `unreadCount` counting only unread statuses
  - `tabTitle` at 0, 1, 2, 9, 10 and 12
- New `extension/src/sessionStore.test.ts` runs against temp dirs and covers:
  - read and write round-trip
  - temp files ignored
  - `isAlive` for `process.pid` (alive), a pid that does not exist, `0`, `-1` and `1.5`
  - live detection using `process.pid` (alive) and a pid that does not exist, with the map value being the file's `cwd`, `null` when the file has no `cwd`, and a dead file sharing an id not overriding a live one
  - `null` for a missing sessions dir
  - `resolveCwd`: a symlinked temp dir resolves to its real path (on macOS `os.tmpdir()` itself sits under the `/var` to `/private/var` link), a missing path comes back unchanged, and `null` stays `null`
  - prune removes only old dead files and does nothing when live is null
- New `extension/src/windowRegistry.test.ts` runs against a temp dir with an injected `isAlive` and covers:
  - publish then read round-trip
  - a dead pid's file is left out and deleted
  - malformed files and a pid that differs from the file name are skipped
  - `unpublishWindow` removes the file and ignores a missing one
  - `knownWindows` replaces a stale self entry with the in-memory folders
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
