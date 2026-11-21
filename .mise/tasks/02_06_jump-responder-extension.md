# 02_06 Extension answers jump requests

## Goal

Every VS Code window watches AYDY's jump directory. The window that owns a requested session claims the request, brings the session's tab forward, and writes a reply telling the app what to raise.

## Files to modify/create

- extension/src/jumpRequests.ts (new)
- extension/src/extension.ts

## Background

- Task 02_05 made the app write `<data>/jump/<id>.json` containing `{"id","sessionId","cwd","tabLabel": string|null,"requestedAt": ms}`. The app then polls for up to 3 s (plus a 1 s grace period after a late claim) for `<data>/jump/<id>.reply.json` containing `{"open": "<abs path>", "reveal": RevealOutcome}`. It runs `code <open>` and deletes the reply.
- `<data>` paths come from `jumpDir()` in `extension/src/aydyPaths.ts` (task 01_01). Atomic writes come from `writeAtomically` in `extension/src/atomicWrite.ts` (task 01_02).
- Ownership: `ownsSession(cwd, folders)` in `extension/src/sessionState.ts` (task 01_04). `extension.ts` keeps a realpath'd folder list (task 01_05).
- Reveal: `revealSession(request, environment)` and `RevealOutcome` (`extension/src/revealSession.ts:62`, `:10-11`), with `revealEnvironment` at `extension/src/extension.ts:75-100`. It already refuses when the tab is not present or the Claude Code version is unverified, so it never opens a second copy of a session.
- `open` is the window's generated workspace file when it has one, otherwise the owning folder. Use `vscode.workspace.workspaceFile` when its scheme is `file`. `code <file.code-workspace>` focuses the window holding that workspace. (Task 01_07 added `isGeneratedWorkspace`. Any file-scheme workspace file is fine here.)
- Watch and debounce pattern: `extension/src/windowTracker.ts` (task 01_05).

## Guides

None match.

## Implementation details

- `jumpRequests.ts`:
  - `parseJumpRequest(text): JumpRequest | null` is strict on types and validates `id` against `^[A-Za-z0-9_.-]{1,128}$`.
  - `shouldHandle(request, folders, now)` is `ownsSession(request.cwd, folders) && now - request.requestedAt <= 10_000`.
  - `class JumpResponder` takes options `{dir, folders: () => string[], open: () => string | null, reveal: (r: RevealRequest) => Promise<RevealOutcome>, pollMs = 1000}`.
    - `start()`: `mkdir -p`, then `fs.watch` plus polling.
    - A scan reads every `*.json` that does not end in `.reply.json`. For each request it should handle:
      1. Claim it with `fs.promises.unlink`. `ENOENT` means skip.
      2. Run `reveal({sessionId, tabLabel})` when `tabLabel !== null`, otherwise use `'notPresent'`.
      3. Write `<id>.reply.json` atomically with `{open: open() ?? owningFolder, reveal}`.
    - It handles each id at most once and never overlaps scans.
- `extension.ts`: create the responder with:
  - `open`: the `workspaceFile` fsPath when it uses the file scheme, else null, in which case the responder falls back to the owning folder
  - `reveal`: `(r) => revealSession(r, revealEnvironment)`

  Dispose it through `context.subscriptions`.

## Gotchas

- Two windows whose folders nest (`/a` and `/a/wt`) can both own a cwd. Unlinking makes only one of them act. If the wrong one wins, `revealSession` returns `notPresent` and opens nothing. Leave this edge as is.
- Requests older than 10 s are stale (the app has given up). Ignore them and do not delete them. The app deletes its own requests on timeout.
- Never throw from the watcher. Swallow IO errors until the next scan.

## Verification

- New `extension/src/jumpRequests.test.ts` uses temp dirs and fake timers. It covers:
  - Parse rejects bad shapes and ids.
  - `shouldHandle` checks the folder and age.
  - The responder claims an owned request, calls `reveal` with the label, and writes a reply with `open`.
  - It falls back to the folder when `open()` is null.
  - A null `tabLabel` replies `notPresent` without calling `reveal`.
  - A foreign request is left untouched.
  - When two responders race on one request, only one replies.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
