# Brief: move AYDY's core into the extension, add a count to VS Code window tabs

## Goal

Two outcomes:

1. Each VS Code window tab shows how many of its Claude Code sessions need
   attention, for example `● 2 okven`, `● okven`, or `○ okven` for none.
2. Session tracking works with the AYDY app closed. The extension becomes the
   product and the app becomes an optional viewer.

The owner runs VS Code with `"window.nativeTabs": true` on macOS, one window
per git worktree. Each window is a native tab whose label is the window title.

## Why change the architecture

Today hooks POST to the app's listener, so nothing is tracked when the app is
not running. The extension also runs once per window, so no single copy can be
the authority either. State therefore moves into files that hooks write and
everything else reads.

## Target design

| Piece          | Owns                                                                   |
| -------------- | ---------------------------------------------------------------------- |
| Hook script    | Writes one status file per session (working, needs input, done)        |
| Extension      | Installs hooks, writes the tab count, marks sessions seen, jump to tab |
| App (optional) | Menu bar list, sounds. Reads the files only                            |

Rules already decided with the owner:

- Hooks become command hooks that run a small script. They must work with
  neither the app nor VS Code running.
- Status and "seen" live in separate files, because hooks write one and readers
  write the other. A session is unread when its last event is newer than its
  last seen time.
- Only the extension installs hooks. The app drops its installer.
- The extension matches sessions to its window by folder (the session's working
  directory is inside a workspace folder), not by process tree.
- The app never asks a running extension for data. With the extension absent it
  shows an "install the extension" state.
- The app hides sessions whose process is no longer alive when it reads the
  files. It does not rely on the extension to clean up.
- Tab count: the number of unread sessions in that window. Arriving at the
  window clears it. The label after the count is the folder name, which for a
  worktree is the worktree folder.

## The hard part: writing the tab title

The only way to change a window tab's text is the `window.title` setting.
There is no extension API for it as far as we know; confirm this first.

- VS Code reads a per-window `window.title` from the folder's
  `.vscode/settings.json` or from a `.code-workspace` file. The first is
  tracked by git in most repos, so writing a changing count there dirties the
  repo. That is not acceptable.
- Chosen approach: when a window was opened as a plain folder, the extension
  generates a `.code-workspace` file outside the repo and reopens the same
  window from it. The count is written there.
- In workspace-file mode VS Code ignores window-level settings from the
  folder's `.vscode/settings.json`. The extension must copy that file's
  settings into the generated workspace file and regenerate when they change.
  Verify this behavior before building on it.
- Expect the workspace name to gain a "(Workspace)" suffix in title variables,
  so write the folder name as literal text.

Rejected, do not revisit:

- A git clean filter that strips the title line. Tested in a throwaway repo:
  git still reports the file modified whenever its size changes, and a merge
  touching the file is refused.
- The skip-worktree flag. It collides with any real change to the file.

## Verified facts

- In the Claude Code VS Code extension 2.1.328, `Stop` fires, and
  `Notification` fires with `permission_prompt` when a permission or question
  dialog opens. The `idle_prompt` variant was not tested.
- Hook changes in a settings file took effect in a running session without a
  restart.

## Details to get right

- Hook installation runs in every window at once. Check first and write only
  when something is missing, so windows do not race on
  `~/.claude/settings.json`.
- The extension's install folder changes name with each version. Copy the hook
  script to a fixed path and refresh it on activation.
- The hook entry must do nothing if the script is missing, and uninstalling the
  extension should remove the entries.
- Jump to a session is the one place the app must reach a running window.
  Decide whether that stays on the existing loopback channel or becomes a
  request file.

## Open decision for the owner

Today a session is marked unread unless its own tab is in front in a focused
window. For the tab count the owner leaned toward a simpler rule: a session
that finishes while its window has focus is not counted. Confirm which rule
applies before implementing.

## Order of work

1. Hook script, file format, hook installation in the extension, and the tab
   count. This ships alongside the current app; keep the existing hook entries
   so both are fed.
2. Switch the app to read the files.
3. Remove the listener, port file, registry, process-tree matching and hook
   installer from the app.

## Investigation

1. Extension entry point is extension/src/extension.ts:120 (`activate`). Folders come from `workspaceFolders` at extension.ts:54. Focus and the active Claude tab are read at extension.ts:102-111 and fed into FocusReporter (extension/src/focusReporter.ts:41, 150 ms debounce at :27). New file and tab-count logic should follow this split: logic in a pure module with colocated `*.test.ts` (as in revealSession.ts / focusReporter.ts), wiring stays in extension.ts.
2. Packaging: extension/.vscodeignore:4 excludes `src/**`, so a hook script kept under src/ will not ship in the VSIX. extension/package.json has `contributes: {}` and `onStartupFinished` only. There are no extension scripts; the root package.json `build-extension`, `build-tsc` and `test` cover extension/ (vitest include at vitest.config.ts:19).
3. Pinned API is `@types/vscode` 1.90.0 (root package.json devDeps; extension/package.json `engines` ^1.90.0). Nothing in extension/src touches `window.title`, `workspaceFile` or `.code-workspace` yet, so the "no title API" check is still open.
4. Conflict risk: the app's installer treats any command hook whose text contains `X-Aydy-Hook` as its own and deletes it on reinstall or uninstall (src-tauri/src/hooks/merge.rs:468-482, :171). The new script entries must not contain that marker. It also takes over bare `afplay` commands (merge.rs:486-514).
5. Settings-write pattern to mirror in TS: round-trip the whole JSON, write atomically, abort if the file changed on disk (src-tauri/src/hooks/settings_file.rs:186-205, atomic write at src-tauri/src/atomic_file.rs:20). Honor `CLAUDE_CONFIG_DIR` (settings_file.rs:22, :229-239). extension/src has no equivalent today.
6. Hook payload fields the code already relies on: `session_id`, `hook_event_name`, `cwd`, `transcript_path`, `message`, `notification_type`, with fallback from message to type (src-tauri/src/listener/routes.rs:43-80). SessionEnd deletes the session (src-tauri/src/session/registry.rs:195-199).
7. Liveness: pids come from `~/.claude/sessions/<pid>.json` checked against the process table (src-tauri/src/discovery/session_files.rs:1-35). Hook payloads carry no pid, so status files need another source for liveness.
8. Current unread rule in code: Stop/Notification mark a session unread unless it is the focused window's active tab (registry.rs:201-218, is_watched at :675). A focused window clears all of its sessions (registry.rs:392-394, read_window at :409), which already matches "arriving at the window clears it".
9. Existing data path: app data dir `~/Library/Application Support/app.aydy` (extension/src/aydyClient.ts:12, :100-101; src-tauri/tauri.conf.json:5).
10. Jump today goes app -> extension over the loopback channel (aydyClient.ts plus src-tauri/src/extension/command.rs), executed by revealSession (extension.ts:67-92, :131).
11. Hard to undo: writes to the user's `~/.claude/settings.json`, raced by every window. Reopening a folder window as a generated `.code-workspace` (e.g. `vscode.openFolder`) restarts that window's extension host, which likely kills running Claude Code sessions in it — unverified. Uninstall cleanup of hook entries is a one-way edit to user config.
12. Stage 1 leaves the app alone: its HTTP entries in HTTP_EVENTS (merge.rs:34) and SessionStart curl (merge.rs:101) stay. Stages 2-3 remove src-tauri/src/listener/, src-tauri/src/port_file.rs, src-tauri/src/extension/registry.rs, src-tauri/src/discovery/process_tree.rs, src-tauri/src/hooks/install.rs, and UI in src/components/organisms/HookSettings.vue and src/components/organisms/ExtensionPrompt.vue.
13. Nothing in the code contradicts the brief.

## Decisions

1. Unread rule: a session whose Stop or Notification arrives while its window has focus is not counted (window-focus rule, not per-tab). Arriving at a window clears all its sessions.
2. Reopening a plain-folder window from a generated `.code-workspace` happens only at the owner's choice: the extension offers it via a notification with a button, and a command runs it later. Never automatic.
3. Scope: all three stages in this run — hook script and files plus extension install and tab count; the app switched to reading the files; then the listener, port file, registry, process-tree matching and hook installer removed from the app.

## Assumptions

- Jump to a session becomes a request file: the app writes it, every extension watches the directory and the one whose window owns the session acts on it. The loopback channel goes away with the listener in stage 3.
- Status, seen and request files live under the existing app data namespace `~/Library/Application Support/app.aydy/`, created by whichever writer runs first; the hook script must not depend on the app having created it.
- Liveness for the app comes from matching `session_id` against `~/.claude/sessions/<pid>.json` and checking the pid, as the app does today, unless the planner finds a more reliable source in the hook environment.
- `Notification` with `idle_prompt` is treated as needs input, like `permission_prompt`.
- SessionEnd removes the session's status file.
- The app shows the "install the extension" state when the fixed-path hook script or its hook entries are absent.
- The "no title API" claim and the claim that workspace-file mode ignores window-level settings from `.vscode/settings.json` are verified against the pinned VS Code API and docs before the title work is built on them.
- Generated `.code-workspace` files live under the app data namespace, one per folder, outside any repo.
- The existing app hook entries keep working through stage 1 and are removed by the app-side cleanup in stage 3; the new script entries never carry the `X-Aydy-Hook` marker.
- Mock: the reopen offer shows once per folder, warns that reopening may end running Claude Code sessions, and is backed by the palette command "AYDY: Show Session Count in Tab Title". Windows opened from the user's own `.code-workspace` are not covered by the mock.
- Mock: the "Install the extension" screen replaces the app's hook-installing Setup screen; install steps assume a VSIX. Settings loses its Hooks section and gains a "Session tracking: Installed" row.
- Mock: the app drops "you are here", the Reconnecting/stale and port-move notices, and the icon's not-receiving state. A jump no window answers shows "No VS Code window took the jump".

- Mock: the tab count caps at `9+` (10 or more unread shows `● 9+ folder`). The popover header's own count ("5 sessions · 3 new") is not the tab count and stays uncapped.

## Proposal

Issue: tracking dies with the app closed, and VS Code window tabs show no attention count.
Approach: hooks run a script that writes per-session status files; the extension installs the hooks, tracks seen state, and writes `● N folder` (capped at `9+`) into a generated workspace file's `window.title` (reopen offered, never forced); the app becomes a file reader, jump moves to a request file, and the listener, registry and installer are removed.
Skips: none — spec and critic both run (cross-module, user-config writes).
