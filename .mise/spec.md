# Spec: move AYDY's core into the extension, add a count to VS Code window tabs

## What

- Claude Code hooks run a small shell script installed by the VS Code extension. The script writes one status file per session. It works with neither the AYDY app nor VS Code running.
- Each VS Code window shows how many of its sessions are unread in its tab title: `○ okven` for none, `● okven` for one, `● N okven` for 2 to 9, `● 9+ okven` for 10 or more. The name is the folder name written as literal text.
- A session belongs to exactly one window. Of all open windows with a workspace folder that contains the session's directory, the owner is the one whose containing folder is the most specific (longest). Ties go to the lowest window pid. The session's directory is its **launch** directory (see "Session directory"), not the hook payload's `cwd`, which moves when the session changes directory. Each window publishes its folders in `<data>/windows/<pid>.json`, so every extension can tell whether a more specific window exists. The tab count, marking seen on focus, and claiming a jump all use this one rule.
- The unread rule, used identically by the extension (`isUnread`) and the app (`Session::unread`): a session is unread when its **last** event is `Stop` or `Notification` and that event's time (the status file's mtime) is newer than its seen time (0 when there is no seen file). Any later event (`UserPromptSubmit`, `SessionStart`) makes it not unread, whatever the seen time.
- Remote windows (Remote-SSH, dev containers, WSL) are out of scope. The extension declares `"extensionKind": ["ui"]`, so it always runs on the local machine and never edits a remote host's `~/.claude/settings.json`, and only `file`-scheme workspace folders count as a window's folders, so a remote window owns no sessions.
- A focused window marks all of its sessions seen, both when it gains focus and whenever one of its sessions changes while it has focus. So arriving at a window clears its count, and a session that finishes while its window has focus is never counted (owner decision 1).
- The title can only be written into a `.code-workspace` file. For a window opened as a plain folder, the extension generates one outside the repo. It offers to reopen the window from that file once per folder, and the command `AYDY: Show Session Count in Tab Title` does the same later. It never reopens on its own (owner decision 2). The generated file carries a copy of the folder's `.vscode/settings.json` and is re-synced when that file changes. Windows opened from the user's own `.code-workspace` get no title.
- The app becomes a reader. It builds its list from the status and seen files, and hides sessions whose process is not alive. A jump writes a request file that the owning window answers. When the hook script or its entries are missing, the app shows an "Install the extension" screen instead of the list.
- Removed from the app: the HTTP listener, the port file, the extension registry and loopback channel, process-tree window matching, the hook installer and its UI, the stale and port-move notices, the "you are here" mark, and the icon's not-receiving state. The VS Code extension loses its loopback client and focus reporter.
- All three stages land in this run (owner decision 3).

## Design

### Shared files

Everything lives under `~/Library/Application Support/app.aydy/`, the app's existing data and config directory (`src-tauri/tauri.conf.json:5`, `extension/src/aydyClient.ts:12` and `:101`). Below it is called `<data>`. Every writer creates the directories it needs with `mkdir -p` / `recursive: true`, and nothing depends on the app having run.

| Path | Writer | Content |
| --- | --- | --- |
| `<data>/bin/aydy-hook.sh` | extension, on every activation, written only when the content differs | the hook script, mode 755 |
| `<data>/status/<session_id>.json` | hook script | the raw hook payload JSON exactly as Claude Code sent it on stdin. The file's **mtime is the event time**. |
| `<data>/seen/<session_id>.json` | extension (focus), app (`mark_read`) | `{"seenAt": <epoch ms>}` |
| `<data>/jump/<id>.json` | app | `{"id": string, "sessionId": string, "cwd": string, "tabLabel": string \| null, "requestedAt": <epoch ms>}` |
| `<data>/jump/<id>.reply.json` | extension that claimed `<id>` | `{"open": <absolute path to raise>, "reveal": RevealOutcome}` |
| `<data>/windows/<pid>.json` | the extension host of one VS Code window (`pid` = its `process.pid`), on activation and on folder changes; deleted on dispose, and by any reader that finds the pid dead | `{"pid": <number>, "folders": [<realpath>, ...]}` |
| `<data>/workspaces/<hash>/<folder name>.code-workspace` | extension | generated workspace, see below |
| `<data>/notices/hook-install-failed` | extension | empty marker created with flag `wx`, so only one window shows the failure |

All JSON writes are atomic: write a temp file in the same directory, then rename it. The script uses `mv -f`. The extension uses `extension/src/atomicWrite.ts`. Rust uses `src-tauri/src/atomic_file.rs:29`.

Like the Rust writer (`atomic_file.rs:57-59`), the extension's writer keeps the target's existing permissions when no mode is given, so rewriting a 0600 `~/.claude/settings.json` (which can hold `env` secrets) never widens it to the umask default. When the target is a symlink (dotfiles managers), it resolves the link and writes to the real path, so the link survives. It creates the real target's parent directory (`recursive: true`) before writing, as `src-tauri/src/atomic_file.rs:32` does, so a machine without `~/.claude/` installs cleanly.

A dangling symlink is treated as its missing target, in both settings writers. The extension's install creates the link's target (and its parent directories) and leaves the link in place, so the link then resolves. The app's legacy cleanup (03_04) sees a missing file, returns 0 and creates nothing, as it does for any missing settings file.

### Session directory

Claude Code reports its current directory in each hook payload's `cwd`, so that value moves when a session changes directory, for example a session in window `/a` that enters `.claude/worktrees/x`. The app registry also overwrites `session.cwd` from every event (`src-tauri/src/session/registry.rs:208-209`). Ownership therefore keys on the launch directory:

- The session directory is the `cwd` field of the live `~/.claude/sessions/<pid>.json` whose `sessionId` matches (real shape: `REAL_SESSION_FILE`, `src-tauri/src/discovery/mod.rs:48-60`; live means its `pid` is alive).
- Only when no live session file for that id carries a non-empty `cwd` (or the sessions directory is unreadable) does it fall back to the payload's `cwd`.
- Before any ownership comparison, the extension resolves the session directory with `fs.realpath`, falling back to the raw value when that fails (a deleted directory), because window folders are compared as realpaths.
- The extension uses it for the tab count, seen writes and the jump claim. The app puts it in the jump request's `cwd`. The extension also looks it up again for each request, so a request carrying a payload `cwd` (an older app) is still claimed by the right window.
- That the session file's `cwd` stays fixed for the life of the session is checked by 01_04's implementer from the real files, read-only. If the evidence shows it follows the session's current directory, 01_04 stops and reports `Blocked:` with the evidence, and no later task proceeds on this rule. If no live session has changed directory, the evidence is inconclusive, 01_04 proceeds and says so in its report, and the owner checks it at acceptance (see "Verification boundary").

### Hook script (`extension/hook/aydy-hook.sh`)

The script is POSIX `sh` with no dependencies. It is kept outside `src/` because `extension/.vscodeignore:4` drops `src/**` from the VSIX.

1. It copies stdin with `cat` straight into `<data>/status/.hook.$$.tmp`, so the stored bytes are exactly the input, trailing newlines included (`$(cat)` would strip them).
2. It extracts `session_id` and `hook_event_name` from that temp file with `sed`, matching `"session_id"[[:space:]]*:[[:space:]]*"([A-Za-z0-9_-]{1,128})"`. A key that appears inside a JSON string value is escaped as `\"session_id\"`, so it never matches.
3. When the id is missing it deletes the temp file and exits 0.
4. On `SessionEnd` it runs `rm -f` on the temp file and on `<data>/status/<id>.json`.
5. Otherwise it runs `mv -f` from the temp file onto `<id>.json`.
6. It always exits 0 and prints nothing. `SessionStart` stdout would be injected into the session.

Readers derive status from the payload's `hook_event_name`, using the existing mapping (`src-tauri/src/listener/routes.rs:61-86`):

- `UserPromptSubmit` means working.
- `Notification` means needs input. Every `notification_type` counts, including `idle_prompt`.
- `Stop` means done.
- `SessionStart` means done, but not unread.
- An unknown event name is ignored.

### Hook entries in `~/.claude/settings.json`

The path is `$CLAUDE_CONFIG_DIR/settings.json` when that variable is set and non-empty, otherwise `~/.claude/settings.json`.

For each of `SessionStart`, `UserPromptSubmit`, `Notification`, `Stop` and `SessionEnd`, the extension adds one group:

```json
{ "hooks": [ { "type": "command", "command": "<CMD>", "timeout": 5 } ] }
```

The timeout is 1 for `SessionEnd`. `<CMD>` is exactly:

```sh
f="$HOME/Library/Application Support/app.aydy/bin/aydy-hook.sh"; [ -f "$f" ] && /bin/sh "$f"; exit 0
```

- An entry is AYDY's when its command contains the marker `app.aydy/bin/aydy-hook.sh`. It never contains `X-Aydy-Hook` (`src-tauri/src/hooks/merge.rs:468`) and never starts with `afplay`.
- Install is check-first. The extension reads and parses the file, and if every event already has an entry whose command equals `<CMD>`, it writes nothing. Otherwise it removes marker entries whose command differs, adds the missing ones, and writes atomically.
- Just before the rename it re-reads the file. If the bytes changed, it aborts and retries the whole step once, so concurrent windows converge.
- A missing file is treated as `{}`. Invalid JSON is never written over and is reported.
- Uninstall (`vscode:uninstall`) removes every marker entry and drops groups and events that end up empty. It leaves the script in place, which matches mock state C.
- The app's legacy HTTP and curl entries stay in place through stages 1 and 2. They are removed in stage 3 by a one-time-per-start cleanup in the app (`hooks/legacy.rs`), which strips entries matching `merge::is_aydy_hook`.
- The cleanup resolves a symlinked settings file to its real path before reading and writing, so the link survives, as the extension's writer does. `atomic_file::write_atomically` renames onto the path it is given (`atomic_file.rs:60`), so it must be given the resolved path.
- That cleanup does **not** restore the `afplay` entries the old installer stashed in `removed-hooks.json` (owner decision 4). Sounds belong to the app, and restoring them would double every sound. The cleanup deletes the stash file `<app config dir>/removed-hooks.json`.

### Extension modules (`extension/src/`)

Logic lives in pure modules with colocated `*.test.ts`, following `revealSession.ts` and `focusReporter.ts`. The `vscode` API is touched only in `extension.ts` and in thin environment objects passed in, as with `revealEnvironment` at `extension.ts:67-92`.

- `aydyPaths.ts`: `aydyDataDir()` and the derived dirs and paths (including `windowsDir()`), `claudeConfigDir()`, `claudeSessionsDir()`, `claudeSettingsPath()`, and `HOOK_SCRIPT_MARKER`.
- `atomicWrite.ts`: `writeAtomically(path, text, mode?)` and `writeIfUnchanged(path, expected, text)`. Both resolve a symlinked target to its real path (a dangling link to its missing target), create the real target's parent directory, and without a `mode` copy the existing target's permissions onto the temp file before the rename.
- `hookSettings.ts`: pure JSON transforms, `hookCommand()`, `HOOK_EVENTS`, `hasAllHooks(doc)`, `withAydyHooks(doc)` and `withoutAydyHooks(doc)`.
- `hookInstall.ts`: the IO steps `installHookScript(extensionPath)`, `ensureHooks()` and `removeHooks()`. The result type is `{ok: true, changed: boolean} | {ok: false, reason: 'malformed' | 'unexpected' | 'unreadable' | 'unwritable' | 'changedOnDisk'}`.
- `hookNotice.ts`: claims and releases the cross-window failure marker.
- `uninstall.ts`: the `vscode:uninstall` entry, which calls `removeHooks()`.
- `sessionState.ts`, pure:
  - `parseStatus(text, mtimeMs)` returns `{sessionId, event, cwd, eventAt} | null`.
  - `isUnread(status, seenAt)`.
  - `sessionDir(status, launchCwds)`: `launchCwds?.get(status.sessionId) ?? status.cwd`, the unresolved session directory.
  - `containingFolderLength(cwd, folders)`: the length of the longest folder that equals or contains `cwd` on path segments, or `-1`.
  - `ownerOf(cwd, windows: WindowFolders[])`: the pid of the window with the longest containing folder, ties to the lowest pid, or `null`. `WindowFolders` is `{pid: number, folders: string[]}`. Callers pass a resolved session directory.
  - `unreadCount(statuses, seen)`, counting the unread among statuses the caller already filtered to live and owned, and `tabTitle(count, name)`.
- `sessionStore.ts`, IO:
  - `readStatuses()`, `readSeen()`, `writeSeen(id, ms)`.
  - `isAlive(pid)`: `process.kill(pid, 0)` succeeds or throws `EPERM`; pids that are not positive integers are dead.
  - `readLiveSessions()`, which reads `~/.claude/sessions/*.json` (`sessionId`, `pid`, `cwd`), checks each pid with `isAlive`, and returns a `Map` from live session id to its launch `cwd` (or `null` when the file has none). It returns `null` when the directory is unreadable.
  - `resolveCwd(cwd)`: `fs.realpath`, the raw value when that fails, `null` for `null`.
  - `pruneStale()`, which deletes status and seen files older than 7 days for sessions that are not live, and only runs when the live set is known.
- `windowRegistry.ts`, IO:
  - `publishWindow(pid, folders)` and `unpublishWindow(pid)` write and delete `<data>/windows/<pid>.json`.
  - `readWindows()` returns the published windows whose pid `isAlive`, and deletes the files of dead ones.
  - `knownWindows(selfPid, selfFolders)` is `readWindows()` minus any entry for `selfPid`, plus `{pid: selfPid, folders: selfFolders}` from memory, so a window never depends on reading its own file back.
- `windowTracker.ts`: a class that is given `pid`, `folders()`, `focused()`, `onCount(count)` and optional data dirs (defaulting to the real ones; tests always pass temp dirs). It watches `status/`, `seen/` and `windows/` with `fs.watch`, polls every 5 s as a fallback and for liveness, and debounces by 150 ms (as `focusReporter.ts:27` does). On each pass it:
  1. Reads the statuses, the live sessions with their launch directories, and the known windows, and keeps the live statuses whose `ownerOf(await resolveCwd(sessionDir(status, live)), windows) === pid`.
  2. If the window is focused, writes `seenAt = Date.now()` for each one whose `eventAt > seenAt`.
  3. Calls `onCount` when the count changed.

  A session counts only when it is live, so a crashed session never sticks.
- `jsonc.ts`: `parseJsonc(text)`, which strips comments and trailing commas. Nothing is bundled and `vsce package --no-dependencies` ships no node_modules, so there can be no npm runtime dependency.
- `workspaceFile.ts`:
  - `generatedWorkspacePath(folder)`, where `<hash>` is the first 12 hex characters of the sha256 of the folder's realpath.
  - `isGeneratedWorkspace(uri)`.
  - `buildWorkspace(existing, folder, folderSettings, title)`, which is pure.
  - `writeWorkspace(...)`, `setWorkspaceTitle(path, title)` and `syncFolderSettings(path, folder)`, which do read-modify-write with an atomic write.
- `reopenOffer.ts`: the once-per-folder offer, keyed in `globalState`, and the reopen action, `vscode.openFolder` with `{forceReuseWindow: true}`.
- `jumpRequests.ts`: `parseJumpRequest`, `shouldHandle(request, selfPid, windows, now)` (`ownerOf(request.cwd, windows) === selfPid`, ignoring requests older than 10 s; the responder first replaces `request.cwd` with the resolved session directory, `resolveCwd(launch.get(sessionId) ?? request.cwd)`) and a responder class that watches `jump/`.

Generated workspace file:

```json
{
  "folders": [{ "path": "/abs/folder" }],
  "settings": { "...every key from <folder>/.vscode/settings.json...": "", "window.title": "● 2 okven" },
  "aydy": { "folder": "/abs/folder", "copiedSettings": ["editor.tabSize"] }
}
```

- On re-sync the extension removes the keys previously listed in `aydy.copiedSettings`, adds the folder file's current keys, and keeps `window.title` and anything the user added through the Settings UI.
- The extension writes the title only in a window whose `vscode.workspace.workspaceFile` is under `<data>/workspaces/`. It writes only when the title changed, and the file is read fresh on every write.
- The title is literal text, so the `(Workspace)` suffix never appears.

Verified so far: the `window` namespace in the pinned `@types/vscode` 1.90.0 (`node_modules/@types/vscode/index.d.ts:10343-11100`) has no title API. `workspace.workspaceFile` exists (`index.d.ts:13080`). That workspace-file mode ignores window-scoped settings in `.vscode/settings.json` is still unverified. The owner checks it at acceptance (see "Verification boundary"). Either way the copy is harmless.

### Jump

App side (`src-tauri/src/jump/mod.rs`, rewritten):

1. If `raiser.available()` is false, return `NoWindow` as today.
2. Otherwise, with the target's `cwd` already set to the session's launch directory (`JumpTarget::with_launch_cwd`, from the live session file, falling back to the registry's `session.cwd`; see "Session directory"), write `jump/<id>.json` and poll every 50 ms, for up to 3 s, for `jump/<id>.reply.json`.
3. On a reply, read and delete it. If `reveal == "duplicated"`, return `Duplicated`. Otherwise `raise(open)` and return `Jumped`.
4. On timeout, unlink the request. If that unlink fails with `NotFound`, a window claimed it at the last moment, so wait 1 s more for the reply. Otherwise return `Unanswered`.

`JumpOutcome` becomes `Jumped | NoWindow | Unanswered | Duplicated`. `Raised` and `Unconfirmed` go, and so do `IdeLocks`, `WindowFolders` and `JumpWatch`.

Extension side:

1. Only the window that owns the session by the ownership rule (`ownerOf(resolved session directory, knownWindows(...)) === process.pid`) claims the request, by unlinking it. A parent window never claims a session that a more specific window (for example a worktree under `.claude/worktrees/`) owns, so the tab-label check in `revealSession` (`extension/src/revealSession.ts:122`, labels are truncated titles and not unique) is never the only guard against opening a second live copy of a session (`revealSession.ts:9-16`). An `ENOENT` on unlink means another window took it, which only happens when two windows briefly disagree about the window list.
2. It runs `revealSession` (`extension/src/revealSession.ts:115`) when `tabLabel` is non-null, and records `notPresent` otherwise.
3. It writes the reply, with `open` = the workspace file path when the window has one, otherwise the window's longest folder containing the resolved session directory.

`code <workspace file>` focuses the window already holding that workspace. For plain-folder windows, `code <folder>` behaves as today.

### App

- **Feed**: a new `discovery/file_feed.rs` loop runs every 500 ms.
  - It reads `status/` and `seen/` through `discovery/status_files.rs`.
  - It refreshes the live session id set every 5 s from `session_files::read_all(user_sessions_dir())` plus `LiveProcesses`.
  - It applies a status record only when the session is live and its `(mtime)` differs from the last applied one. A session whose file it applied before and that has now vanished gets `SessionEnd`.
  - It applies seen times.
  - Records carry their recorded time. `apply_status(record, now_ms)` places the event at `clock.now() - (now_ms - written_at_ms)` on the registry's monotonic clock, so a `Stop` from yesterday does not sort as brand new and a `UserPromptSubmit` counts working time from when it happened, not from app launch.
  - Startup is silent. `SoundNotifier` starts unarmed and earns nothing until `arm()`; the first observation after `arm()` is a baseline that earns nothing either. The feed calls `arm()` once its first pass has been applied, so state loaded at startup (including scan-discovered `Unknown` sessions becoming `Done`) never plays a sound, and only changes seen after the initial load do.
  - `HookPayload` and `into_event` move from `listener/routes.rs` to `session/hook_payload.rs` so both use them.
- **Unread**: `Session` gains `attention_at_ms` and `seen_at_ms` (wall-clock epoch ms) and `unread() = attention > seen`. `apply_status` sets `attention_at_ms` to the record's time for `Stop` and `Notification` and clears it to `None` for every other event, so `attention` is set only while the last event is one of those two, and `unread()` is exactly the rule in "What".
  - `SessionRegistry` gains `apply_status(StatusRecord, now_ms)`, `apply_seen(id, ms)` and `mark_read(id, now_ms)`.
  - The focus-driven unread code goes: the `is_watched` gate at `registry.rs:217`, `read_window`, and the clear in `apply_focus_report`. The feed's first pass replays every live status file, so `apply_status` must not stamp records with the registry's `now`.
  - `mark_read` also writes the seen file.
  - Sounds skip sessions that are not unread, in place of `is_current` (`notify/sound.rs:238`). The feed reads `seen/` after `status/` in the same pass.
  - A new `Stop` or `Notification` record is held for one feed pass before it is applied (02_04). The extension writes the seen file about 150 ms after the status file (its debounce), while the feed polls every 500 ms, so without the hold a pass could apply the record before the seen write lands and play a sound for a session that finished in a focused window. On the next pass the seen file is read in the same pass as the held record and applied with it, so the session is already read when the notifier observes it. The first pass holds nothing, because sounds are not armed yet. Other events apply at once.
- **Tracking status**: a new `tracking.rs` exposes the IPC command `tracking_status`, which returns `{state: 'installed' | 'notInstalled' | 'hooksMissing'}`.
  - `notInstalled` means the script is missing.
  - `hooksMissing` means the script is present but some event lacks a marker entry.
  - It reads the settings file through `hooks::settings_file::SettingsFile::user()`.
  - The popover polls it every 2 s while it is not `installed`, and re-reads it each time the popover opens (the popover window gaining focus), so an uninstalled extension shows the install screen on the next open instead of a frozen list.
- **`lock_recovering`** moves from `listener/server.rs:118` to a new `src-tauri/src/sync.rs`. The server re-exports it until the listener is deleted. Each later task that touches an importer switches it to `crate::sync` (02_03 for `notify/sound.rs` and `ipc/commands.rs`, 02_05 for `jump/mod.rs`, 03_03 for `ipc/snapshot_channel.rs`, 03_05 for `discovery/reconciler.rs`). Importers inside modules that are deleted (`extension/`, `hooks/install.rs`) keep the re-export until they go.
- **Test fixtures outside the deleting task's files** are cleaned in the earlier task that owns the file: 02_03 deletes the focus harness and focus tests in `notify/sound.rs`; 02_05 removes the window placement (`ExtensionPid`, `FixedParents`, `window_map::associate`, `apply_window_map`) from the `ipc/commands.rs` `jumpable_session` fixture and the `session.window = Some(..)` line from the `jump/mod.rs` test `a_session_that_cannot_be_jumped_to_has_nothing_to_jump_to`, but keeps the `Registration` import, which the `window` helper (`ipc/commands.rs:954`) and the `extension_status` tests still use; 03_03 removes the installer test helpers, the `extension_status` tests, the `window` helper and the `Registration` import from `ipc/commands.rs`; `ListenerHealth` is deleted in 03_04 with `hooks/install.rs`, whose tests use it; `SessionSnapshot.is_current` is deleted in 03_07, which also updates the fixtures in `tray/icon.rs` and `ipc/snapshot_channel.rs`.

### Verification boundary

The owner runs the installed `/Applications/AYDY.app` against the real home directory while this work is built. Its legacy `X-Aydy-Hook` entries in `~/.claude/settings.json` feed it, and `tauri-plugin-single-instance` (`src-tauri/src/lib.rs:78`) hands a second launch to it. So no implementer verification step in any task may:

- launch the app (`yarn tauri dev`, `yarn tauri build` output, or the installed app) against the real home directory, or quit or restart the owner's installed app;
- install the VSIX into the owner's VS Code, or run `yarn package-extension` (it writes over the owner's `extension/aydy-companion-0.1.0.vsix`);
- read-modify-write, create or delete the real `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR/settings.json`), the real `removed-hooks.json`, or any real file under `~/Library/Application Support/app.aydy/` (status, seen, windows, jump, workspaces, notices, bin), including `touch`.

Tests and substitute checks use scratch directories, a temp `HOME` and an unset `CLAUDE_CONFIG_DIR`. Checks that need the real environment belong to the owner at acceptance:

- the stage-3 app listens on no TCP port, removes the legacy entries and the stash on start, and the popover follows real sessions;
- a plain-folder window accepts the reopen offer, the tab reads `○ <folder>`, a finished session turns it to `● <folder>` while unfocused and focusing clears it, and whether a window-scoped setting in `.vscode/settings.json` applies only through the copy;
- when 01_04 reported the session-file `cwd` check as inconclusive: a session in a repo window that enters `.claude/worktrees/<x>` keeps its `~/.claude/sessions/<pid>.json` `cwd` at the repo folder and stays counted in the repo window's tab, not the worktree window's.

### What gets removed, and when

- **Stage 2**:
  - The listener stops feeding hook events into the registry (02_04). The endpoint still answers 200, because the legacy entries POST until stage 3.
  - Jump stops using the loopback channel and `JumpWatch` (02_05).
  - The popover's gate switches from hook status to tracking status (02_09).
- **Stage 3**, in order:
  1. Listener-era UI (03_01, 03_02).
  2. Snapshot health, the hook and extension IPC commands, and the icon's not-receiving state (03_03).
  3. The installer, the stash and the port file, and `ListenerHealth`, with the legacy cleanup added (03_04).
  4. The listener, the `extension/` module, the reconciler's window association, and the `axum`/`ureq` dependencies (03_05).
  5. The registry's window and focus state, `window_map.rs` and `process_tree.rs` (03_06).
  6. `tab_match`, the event types and `SessionSnapshot.is_current` are trimmed (03_07).
  7. The extension's `AydyClient` and `FocusReporter` (03_08).

Task files list deletions under "Files to delete". Those do not count toward the 5-file limit.

## Hard-to-undo

- The extension writes `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR/settings.json`), the user's own config. It adds and refreshes the five AYDY command-hook entries, raced by every open window. The edit is check-first, atomic, aborts on change and retries once. When the file is missing it creates it, along with `~/.claude/` if absent; when the path is a dangling symlink it creates the link's target and its parent directories (01_02, 01_03).
- Uninstalling the extension runs `vscode:uninstall`, which deletes AYDY's command-hook entries from the user's settings file (01_03).
- On every start, the stage 3 app deletes the legacy `X-Aydy-Hook` HTTP/curl entries from the user's settings file, writing through a symlink's real path so the link survives. It does not restore the stashed `afplay` entries, and it deletes the stash file `removed-hooks.json` from the app config dir, so those entries cannot be restored later (owner decision 4, 03_04).
- Both settings writers rewrite `~/.claude/settings.json` through a symlink's real path and keep the file's permissions: the extension's atomic writer (01_02) and the app's legacy cleanup (03_04).
- Deletes in `<data>`: the extension prunes status and seen files older than 7 days for dead sessions (01_04), window files are deleted on dispose and when their pid is dead (01_04, 01_05), and jump requests and replies are deleted after use (02_05, 02_06). The hook script deletes a status file on `SessionEnd`.
- Reopening a window from a generated `.code-workspace` restarts its extension host and may end Claude Code sessions running in it. It happens only on the owner's click (01_07).
- Removed public surface:
  - The app's loopback HTTP listener and its routes.
  - The `listener.json` port file.
  - The extension's loopback command server.
  - The IPC commands `hook_status`, `preview_hook_install`, `install_hooks`, `reinstall_hooks`, `uninstall_hooks`, `reveal_settings_file` and `extension_status`.

  An older extension or app paired with the new one stops talking to it (02_04, 03_03, 03_05, 03_08).

## Task index

| Task file | What it does | Files |
| --- | --- | --- |
| 01_01_hook-script.md | Hook script and shared path module | extension/hook/aydy-hook.sh, extension/src/aydyPaths.ts |
| 01_02_hook-settings.md | Settings JSON transforms, atomic write, script copy and hook install IO | extension/src/hookSettings.ts, extension/src/atomicWrite.ts, extension/src/hookInstall.ts |
| 01_03_install-on-activation.md | Install hooks on activation, one-window failure notice, uninstall cleanup, local-only `extensionKind` | extension/src/extension.ts, extension/src/hookNotice.ts, extension/src/uninstall.ts, extension/package.json |
| 01_04_session-state.md | Parse status and seen files, liveness and launch directories, window ownership rule and published window list, unread count, title text, pruning | extension/src/sessionState.ts, extension/src/sessionStore.ts, extension/src/windowRegistry.ts |
| 01_05_window-tracker.md | Publish this window's folders; per-window watcher that marks seen on focus and computes the count for the sessions it owns | extension/src/windowTracker.ts, extension/src/extension.ts |
| 01_06_workspace-file.md | JSONC parse and generated workspace file build, sync and title write | extension/src/jsonc.ts, extension/src/workspaceFile.ts |
| 01_07_title-and-reopen.md | Write the title, sync folder settings, reopen offer and command | extension/src/reopenOffer.ts, extension/src/extension.ts, extension/package.json, extension/README.md |
| 02_01_sync-module.md | Move `lock_recovering` to `sync.rs` | src-tauri/src/sync.rs, src-tauri/src/lib.rs, src-tauri/src/listener/server.rs, src-tauri/src/discovery/resume.rs, src-tauri/src/tray/popover.rs |
| 02_02_status-files-reader.md | Shared hook payload parsing and status/seen file reader | src-tauri/src/session/hook_payload.rs, src-tauri/src/session/mod.rs, src-tauri/src/listener/routes.rs, src-tauri/src/discovery/status_files.rs, src-tauri/src/discovery/mod.rs |
| 02_03_registry-seen-unread.md | Unread from attention vs seen times, status records at their recorded time, `mark_read` writes the seen file, sounds skip read sessions and stay silent until armed | src-tauri/src/session/registry.rs, src-tauri/src/session/session.rs, src-tauri/src/notify/sound.rs, src-tauri/src/ipc/commands.rs, src-tauri/src/lib.rs |
| 02_04_file-feed.md | Loop feeding the registry from files, arms sounds after the initial load, listener stops feeding hooks | src-tauri/src/discovery/file_feed.rs, src-tauri/src/discovery/mod.rs, src-tauri/src/lib.rs, src-tauri/src/listener/server.rs, src-tauri/src/listener/routes.rs |
| 02_05_jump-request-app.md | Jump by request and reply files, request `cwd` from the launch directory | src-tauri/src/jump/mod.rs, src-tauri/src/lib.rs, src-tauri/src/listener/server.rs, src-tauri/src/listener/routes.rs, src-tauri/src/ipc/commands.rs |
| 02_06_jump-responder-extension.md | Extension claims and answers jump requests | extension/src/jumpRequests.ts, extension/src/extension.ts |
| 02_07_jump-outcome-ui.md | Popover handles `unanswered` with the new copy | src/types/JumpOutcome.ts, src/composables/useJump.ts, src/components/molecules/JumpFailureNotice.vue, docs/design/menubar-ux.md |
| 02_08_tracking-status-app.md | `tracking_status` IPC command | src-tauri/src/tracking.rs, src-tauri/src/lib.rs, src-tauri/src/ipc/commands.rs |
| 02_09_install-extension-screen.md | Install-the-extension screen and popover gate | src/types/TrackingStatus.ts, src/composables/useTracking.ts, src/components/organisms/InstallExtension.vue, src/App.vue, docs/design/menubar-ux.md |
| 03_01_remove-listener-ui.md | Remove hook, extension-prompt, stale and port-move UI; add the Session tracking row | src/App.vue, src/components/organisms/SettingsPane.vue, src/components/molecules/PopoverHeader.vue, src/composables/useSessions.ts, src/types/SessionList.ts |
| 03_02_drop-you-are-here-ui.md | Remove `isCurrent` and update the UX doc | src/types/Session.ts, src/components/molecules/SessionRow.vue, docs/design/menubar-ux.md |
| 03_03_drop-health-and-hook-ipc.md | Snapshot drops health, port move, stale and extension fields; hook and extension commands go; no not-receiving icon | src-tauri/src/ipc/snapshot_channel.rs, src-tauri/src/ipc/commands.rs, src-tauri/src/tray/icon.rs, src-tauri/src/lib.rs |
| 03_04_remove-installer.md | Delete the installer, stash, port file and `ListenerHealth`; add the legacy hook cleanup that also deletes the stash file | src-tauri/src/hooks/merge.rs, src-tauri/src/hooks/mod.rs, src-tauri/src/hooks/legacy.rs, src-tauri/src/lib.rs, src-tauri/src/listener/server.rs |
| 03_05_remove-listener-and-extension-module.md | Delete the listener and the extension registry/channel | src-tauri/src/lib.rs, src-tauri/src/discovery/reconciler.rs, src-tauri/src/discovery/window_map.rs, src-tauri/src/session/event.rs, src-tauri/Cargo.toml |
| 03_06_remove-window-association.md | Registry and reconciler drop windows; delete window_map and process_tree | src-tauri/src/session/registry.rs, src-tauri/src/session/session.rs, src-tauri/src/discovery/reconciler.rs, src-tauri/src/discovery/mod.rs |
| 03_07_trim-tab-match-and-events.md | Keep only tab label helpers; drop `WindowId`/`FocusReport` and `SessionSnapshot.is_current` | src-tauri/src/session/tab_match.rs, src-tauri/src/session/event.rs, src-tauri/src/session/registry.rs, src-tauri/src/tray/icon.rs, src-tauri/src/ipc/snapshot_channel.rs |
| 03_08_extension-drop-loopback.md | Extension drops `AydyClient` and `FocusReporter` | extension/src/extension.ts, extension/src/revealSession.ts, extension/package.json, extension/README.md |
