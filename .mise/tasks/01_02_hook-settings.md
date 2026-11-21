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
  - Write atomically (`src-tauri/src/atomic_file.rs:20-29`). It carries the target's existing permissions over to the temp file before the rename (`atomic_file.rs:57-59`), so rewriting a private file does not widen it.
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

- `resolveTarget(path)`: `lstat` the path. When it is a symlink, return `fs.realpath(path)`. When the link dangles (`ENOENT` from realpath), follow it one hop at a time (`path.resolve(dirname(p), readlink(p))`) until the path is missing or not a link, and return that path, the missing target. Give up after 32 hops with an error (callers map it to `unwritable`). Otherwise (including a missing path) return `path` unchanged.
- Dangling symlinks (spec, "Shared files"): a dangling link is treated as its missing target. Install creates that target, and its parent directories, and the link stays a link that now resolves. The app's legacy cleanup (03_04) treats the same case as a missing file and writes nothing. Do not replace the link and do not skip.
- `writeAtomically(path, text, mode?)`:
  1. Resolves the real target with `resolveTarget`, so a symlinked settings file (dotfiles managers) keeps its link and the real file is updated.
  2. Creates the real target's parent directory with `fs.mkdir(dirname(real), {recursive: true})`, as `src-tauri/src/atomic_file.rs:32` does, so a machine without `~/.claude/` (or a dangling link into a missing directory) installs cleanly.
  3. Writes to `<real dir>/.<real base>.<pid>.<random>.tmp` and fsyncs.
  4. Sets the temp file's mode: `mode` when given; otherwise, when the real target exists, its current permission bits (`stat.mode & 0o7777`), mirroring `src-tauri/src/atomic_file.rs:57-59`; otherwise leaves the umask default.
  5. Renames onto the real target, and cleans up the temp file on any failure.
- `writeIfUnchanged(path, expected: string | null, text)` follows the same steps (resolved target, parent directory created, permissions carried over) and re-reads the real target just before the rename, throwing `ChangedOnDisk` if its content differs from `expected`. `null` means the file did not exist.

`hookInstall.ts`:

- `installHookScript(extensionPath, paths?)` reads `<extensionPath>/hook/aydy-hook.sh` and compares it with the installed copy. If they differ, it creates `binDir` recursively and writes the copy atomically with mode `0o755`. It returns whether it wrote.
- `ensureHooks(paths?)` reads the settings file:
  - A missing file is treated as `{}`, and so is a dangling symlink (reading it gives `ENOENT`). Parse failure returns `{ok:false, reason:'malformed'}`.
  - If `hasAllHooks` is true, it returns `{ok:true, changed:false}` without writing.
  - Otherwise it renders `withAydyHooks` with the detected indent (the first indented line's whitespace, default two spaces) and the trailing newline.
  - It writes with `writeIfUnchanged`. On `ChangedOnDisk` it retries the whole read-check-write once.
  - Reasons are `'malformed' | 'unexpected' | 'unreadable' | 'unwritable' | 'changedOnDisk'`.
- `removeHooks(paths?)` follows the same flow with `withoutAydyHooks`. A missing file is a no-op.

## Gotchas

- Every open window calls `ensureHooks` on activation at the same moment. The no-write path when hooks are present is what prevents a race. Never write when nothing changed.
- Do not reorder or reformat keys you did not touch. `JSON.stringify(value, null, indent)` keeps insertion order for string keys.
- The settings file is the user's. Never write over a file that failed to parse.
- `~/.claude/settings.json` is often mode 0600 because it can hold `env` secrets. `ensureHooks` and `removeHooks` pass no mode, so the carry-over in `writeAtomically` is what keeps it private. Do not pass a default mode from those callers.
- Renaming onto a symlink path would replace the link with a regular file. Always rename onto the resolved real path.

## Verification

- New `extension/src/hookSettings.test.ts`. Use a fixture that copies the real settings shape from `REAL_SETTINGS` at `src-tauri/src/hooks/mod.rs:94-178`. That constant holds only `afplay` command hooks, so the fixture must also add, by hand, a legacy AYDY http entry to at least one event, for example `{ "hooks": [ { "type": "http", "url": "http://127.0.0.1:47823/hook", "headers": { "X-Aydy-Hook": "1" } } ] }` under `Stop` (the shape `is_aydy_hook` at `src-tauri/src/hooks/merge.rs:468-482` recognises). Every check below asserts that both the `afplay` hooks and that legacy entry come through untouched, since the app's legacy entries stay until stage 3. Check:
  - add to an empty doc
  - add alongside the user's hooks with them untouched
  - a no-op when present
  - replacing a stale AYDY command
  - removal leaving the user's hooks intact and pruning empty groups and events
  - wrong-shape input throws
- New `extension/src/atomicWrite.test.ts` checks:
  - the write, and an explicit `mode` applied
  - an existing 0600 target stays 0600 after `writeAtomically` and after `writeIfUnchanged` with no mode (set `process.umask(0o022)` for the test and restore it)
  - a symlinked target: after the write, the link is still a symlink (`lstat().isSymbolicLink()`), it points at the same real file, and the real file holds the new text with its permissions kept
  - a `ChangedOnDisk` abort that leaves the file intact
  - a target whose parent directory does not exist (`<tmp>/a/b/settings.json`): `writeAtomically` and `writeIfUnchanged` with `expected: null` create the directories and the file
  - a dangling symlink whose target is in a missing directory: after the write the link is still a symlink, its target and the target's directory now exist, and the target holds the text
  - no leftover temp files
- New `extension/src/hookInstall.test.ts` runs against a temp HOME and checks the items below. Every call passes `paths = {home: <temp dir>, env: {}}`, so `CLAUDE_CONFIG_DIR` is unset and `claudeConfigDir()` can never resolve to a real directory (spec, "Verification boundary"). No test calls `installHookScript`, `ensureHooks` or `removeHooks` without `paths`. It checks:
  - script copied with mode 755, and rewritten only when the source differs
  - `ensureHooks` on a temp HOME with no `.claude/` directory creates `.claude/settings.json` with the five entries
  - `ensureHooks` writes once, then reports `changed:false` with the file mtime untouched
  - malformed JSON is reported and the file is unchanged
  - two concurrent `ensureHooks` calls end with exactly one entry per event
  - a 0600 settings file is still 0600 after `ensureHooks` and `removeHooks` rewrite it
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
