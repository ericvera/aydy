# 02_06 Extension answers jump requests

## Goal

Every VS Code window watches AYDY's jump directory. Only the one window that owns a requested session (by the most-specific-folder rule) claims the request, brings the session's tab forward, and writes a reply telling the app what to raise.

## Files to modify/create

- extension/src/jumpRequests.ts (new)
- extension/src/extension.ts

## Background

- Task 02_05 made the app write `<data>/jump/<id>.json` containing `{"id","sessionId","cwd","tabLabel": string|null,"requestedAt": ms}`. The app then polls for up to 3 s (plus a 1 s grace period after a late claim) for `<data>/jump/<id>.reply.json` containing `{"open": "<abs path>", "reveal": RevealOutcome}`. It runs `code <open>` and deletes the reply.
- `<data>` paths come from `jumpDir()` in `extension/src/aydyPaths.ts` (task 01_01). Atomic writes come from `writeAtomically` in `extension/src/atomicWrite.ts` (task 01_02).
- Ownership (`.mise/spec.md`, "What" and "Jump"): a session belongs to exactly one window, the open window with the longest folder containing its session directory, ties to the lowest pid.
- Session directory (`.mise/spec.md`, "Session directory"): the launch directory from the session's live `~/.claude/sessions/<pid>.json`, falling back to the request's `cwd`, then realpath'd (raw value on failure), because window folders are realpaths. 02_05 already puts the launch directory in the request's `cwd`; the responder looks it up again so a request from an older app, which carries the moving payload `cwd`, is still claimed by the right window.
  - `readLiveSessions()` (live session id to launch `cwd`, or `null` when unreadable) and `resolveCwd(cwd)` in `extension/src/sessionStore.ts` (task 01_04).
  - `ownerOf(cwd, windows)` and `containingFolderLength(cwd, folders)` in `extension/src/sessionState.ts` (task 01_04).
  - `knownWindows(selfPid, selfFolders)` in `extension/src/windowRegistry.ts` (task 01_04) returns every live published window plus this one from memory.
  - `extension.ts` keeps a realpath'd folder list and publishes it under `process.pid` (task 01_05).
- Why ownership matters here: with a repo window `/a` and a worktree window under it (for example `/a/.claude/worktrees/x`), a prefix match lets the repo window claim the worktree's session. `revealSession`'s remaining guard is the tab label (`extension/src/revealSession.ts:122`), and labels are truncated titles, not unique. A matching label in the wrong window makes `claude-vscode.editor.open` resume a second live copy of the session (`revealSession.ts:9-16`).
- Reveal: `revealSession(request, environment)` (`extension/src/revealSession.ts:115`) and `RevealOutcome` (`:63-64`), with `revealEnvironment` at `extension/src/extension.ts:67-92`. It refuses when the tab is not present or the Claude Code version is unverified.
- `open` is the window's workspace file when it has one, otherwise the owning folder. Use `vscode.workspace.workspaceFile` when its scheme is `file`. `code <file.code-workspace>` focuses the window holding that workspace. (Task 01_07 added `isGeneratedWorkspace`. Any file-scheme workspace file is fine here.)
- Watch and debounce pattern: `extension/src/windowTracker.ts` (task 01_05).

## Guides

None match.

## Implementation details

- `jumpRequests.ts`:
  - `parseJumpRequest(text): JumpRequest | null` is strict on types and validates `id` against `^[A-Za-z0-9_.-]{1,128}$`.
  - `shouldHandle(request, selfPid, windows: WindowFolders[], now)` is `ownerOf(request.cwd, windows) === selfPid && now - request.requestedAt <= 10_000`. It is pure, and it is always called with `request.cwd` already replaced by the resolved session directory.
  - `class JumpResponder` takes options `{dir, pid: number, folders: () => string[], windows: () => Promise<WindowFolders[]>, launchCwds: () => Promise<ReadonlyMap<string, string | null> | null>, resolveCwd: (cwd: string | null) => Promise<string | null>, open: () => string | null, reveal: (r: RevealRequest) => Promise<RevealOutcome>, pollMs = 1000}`. `windows` defaults to `() => knownWindows(pid, folders())`, `launchCwds` to `readLiveSessions` and `resolveCwd` to `resolveCwd` from `sessionStore.ts`. `dir` has no default in the class; only `extension.ts` passes `jumpDir()`.
    - `start()`: `mkdir -p`, then `fs.watch` plus polling.
    - A scan reads the known windows and `launchCwds()` once, then every `*.json` that does not end in `.reply.json`. For each request it computes `cwd = await resolveCwd(launch?.get(request.sessionId) ?? request.cwd)` and calls `shouldHandle({...request, cwd}, ...)`. For each request it should handle:
      1. Claim it with `fs.promises.unlink`. `ENOENT` means skip.
      2. Run `reveal({sessionId, tabLabel})` when `tabLabel !== null`, otherwise use `'notPresent'`.
      3. Write `<id>.reply.json` atomically with `{open: open() ?? owningFolder, reveal}`, where `owningFolder` is this window's longest folder containing the resolved `cwd`.
    - It handles each id at most once and never overlaps scans.
- `extension.ts`: create the responder with:
  - `pid`: `process.pid`
  - `folders`: the realpath'd folder list from 01_05
  - `open`: the `workspaceFile` fsPath when it uses the file scheme, else null, in which case the responder falls back to the owning folder
  - `reveal`: `(r) => revealSession(r, revealEnvironment)`

  Dispose it through `context.subscriptions`.

## Gotchas

- A parent window must never claim a session that a more specific window owns. Do not fall back to a prefix match when the windows list is unreadable: `knownWindows` still returns this window, so the worst case is the pre-registry behaviour for windows that never published (an older extension), never a guess.
- Unlinking is still the claim, for the brief moment two windows disagree about the window list (one just opened). Only one can act.
- Requests older than 10 s are stale (the app has given up). Ignore them and do not delete them. The app deletes its own requests on timeout.
- Never throw from the watcher. Swallow IO errors until the next scan.

## Verification

- New `extension/src/jumpRequests.test.ts` uses temp dirs and fake timers. It covers:
  - Parse rejects bad shapes and ids.
  - `shouldHandle` checks ownership and age.
  - Moved session: windows pid 1 at `/a` and pid 2 at `/a/.claude/worktrees/x`; a request whose `cwd` is inside the worktree but whose session's launch `cwd` (injected `launchCwds`) is `/a` is claimed and answered by pid 1 only, with `open` falling back to `/a`. With `launchCwds` returning `null` or no entry, the request's `cwd` decides.
  - Resolution: the injected `resolveCwd` runs before ownership (a fake mapping `/link/src` to `/a/src` makes pid 1 claim it), and with the real `resolveCwd` a request `cwd` under a symlinked temp folder is claimed by the window holding the folder's realpath.
  - Nested windows: with windows `{pid: 1, folders: ['/a']}` and `{pid: 2, folders: ['/a/.claude/worktrees/x']}`, a request whose `cwd` is inside the worktree is handled by pid 2 and not by pid 1; a request with `cwd` `/a/src` is handled by pid 1.
  - Two responders (pids 1 and 2 as above) sharing one jump dir: for the worktree session only pid 2 calls `reveal` and replies, and pid 1 leaves the request untouched.
  - The responder claims an owned request, calls `reveal` with the label, and writes a reply with `open`.
  - It falls back to the owning folder when `open()` is null.
  - A null `tabLabel` replies `notPresent` without calling `reveal`.
  - A foreign request is left untouched.
  - When two responders that both consider themselves the owner race on one request, only one replies.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
