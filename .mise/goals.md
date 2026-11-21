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
