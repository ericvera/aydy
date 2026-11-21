# 01_02 Hook settings transforms and install IO

## Goal

Add the code that copies the hook script to its fixed path and keeps AYDY's command-hook entries in the user's Claude settings file. The edit is check-first and atomic, so many VS Code windows can run it at once safely.

## Files to modify/create

- extension/src/hookSettings.ts (new, pure)
- extension/src/atomicWrite.ts (new)
- extension/src/hookInstall.ts (new, IO)

## Background

- Task 01_01 produced `extension/src/aydyPaths.ts`: `hookScriptPath()`, `binDir()`, `claudeSettingsPath()`, `HOOK_SCRIPT_MARKER` and `HOOK_SCRIPT_SOURCE`. It also produced the script `extension/hook/aydy-hook.sh`.
- Design (`.mise/spec.md`, "Hook entries"): for each of `SessionStart`, `UserPromptSubmit`, `Notification`, `Stop` and `SessionEnd`, append the group `{"hooks":[{"type":"command","command":CMD,"timeout":5}]}`. The timeout is `1` for `SessionEnd`, whose hooks share a 1.5 s machine-wide budget (`src-tauri/src/hooks/merge.rs:39-48`). `CMD` is exactly:
  `f="$HOME/Library/Application Support/app.aydy/bin/aydy-hook.sh"; [ -f "$f" ] && /bin/sh "$f"; exit 0`
- An entry is AYDY's when its `command` contains `HOOK_SCRIPT_MARKER`.
- The AYDY app's own installer still runs in this stage. It deletes any command hook containing `X-Aydy-Hook` (`src-tauri/src/hooks/merge.rs:468-482`) and takes over bare `afplay` commands (`merge.rs:486-514`). Our command contains neither.
- The pattern to mirror is the Rust settings writer:
  - Round-trip the whole JSON.
  - Abort if the file changed on disk (`src-tauri/src/hooks/settings_file.rs:186-205`).
  - Write atomically (`src-tauri/src/atomic_file.rs:20-29`).
  - Keep the file's indentation and trailing newline (`settings_file.rs:42` `JsonStyle::detect`).

## Guides

None match.

## Implementation details

`hookSettings.ts`, pure, no fs:

- `HOOK_EVENTS`.
- `hookCommand()`.
- `isAydyHook(hook: unknown): boolean`.
- `hasAllHooks(doc: unknown): boolean`. True when every event has a hook whose command `=== hookCommand()`.
- `withAydyHooks(doc: Record<string, unknown>): Record<string, unknown>`. Returns a new object: it removes AYDY hooks whose command differs, adds one group per event that lacks an exact entry, and preserves all other keys and their order.
- `withoutAydyHooks(doc)`. Removes every AYDY hook, drops groups whose `hooks` array becomes empty and events whose array becomes empty, and drops `hooks` itself if it becomes `{}`.
- Throws a typed `UnexpectedShape` error when `hooks`, an event's value, or a group's `hooks` is the wrong type. Nothing is guessed.

`atomicWrite.ts`:

- `writeAtomically(path, text, mode?)` writes to `<dir>/.<base>.<pid>.<random>.tmp`, fsyncs, renames, and cleans up the temp file on failure.
- `writeIfUnchanged(path, expected: string | null, text)` re-reads the file just before the rename and throws `ChangedOnDisk` if its content differs from `expected`. `null` means the file did not exist.

`hookInstall.ts`:

- `installHookScript(extensionPath, paths?)` reads `<extensionPath>/hook/aydy-hook.sh` and compares it with the installed copy. If they differ, it creates `binDir` recursively and writes the copy atomically with mode `0o755`. It returns whether it wrote.
- `ensureHooks(paths?)` reads the settings file:
  - A missing file is treated as `{}`. Parse failure returns `{ok:false, reason:'malformed'}`.
  - If `hasAllHooks` is true, it returns `{ok:true, changed:false}` without writing.
  - Otherwise it renders `withAydyHooks` with the detected indent (the first indented line's whitespace, default two spaces) and the trailing newline.
  - It writes with `writeIfUnchanged`. On `ChangedOnDisk` it retries the whole read-check-write once.
  - Reasons are `'malformed' | 'unexpected' | 'unreadable' | 'unwritable' | 'changedOnDisk'`.
- `removeHooks(paths?)` follows the same flow with `withoutAydyHooks`. A missing file is a no-op.

## Gotchas

- Every open window calls `ensureHooks` on activation at the same moment. The no-write path when hooks are present is what prevents a race. Never write when nothing changed.
- Do not reorder or reformat keys you did not touch. `JSON.stringify(value, null, indent)` keeps insertion order for string keys.
- The settings file is the user's. Never write over a file that failed to parse.

## Verification

- New `extension/src/hookSettings.test.ts`. Use a fixture that copies the real settings shape from `src-tauri/src/hooks/mod.rs:579-629` (with `afplay` hooks plus an `X-Aydy-Hook` http hook) and check:
  - add to an empty doc
  - add alongside the user's hooks with them untouched
  - a no-op when present
  - replacing a stale AYDY command
  - removal leaving the user's hooks intact and pruning empty groups and events
  - wrong-shape input throws
- New `extension/src/atomicWrite.test.ts` checks the write, mode bits, a `ChangedOnDisk` abort that leaves the file intact, and no leftover temp files.
- New `extension/src/hookInstall.test.ts` runs against a temp HOME and checks:
  - script copied with mode 755, and rewritten only when the source differs
  - `ensureHooks` writes once, then reports `changed:false` with the file mtime untouched
  - malformed JSON is reported and the file is unchanged
  - two concurrent `ensureHooks` calls end with exactly one entry per event
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
