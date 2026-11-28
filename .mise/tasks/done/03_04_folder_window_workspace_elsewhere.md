# 03_04 Folder window whose workspace is open elsewhere

## Goal

When a VS Code window opened as a plain folder has that folder's AYDY `.code-workspace` already open in another window, the extension tells the owner so instead of calling `vscode.openFolder` — which cannot reuse this window and only focuses the other one.

## Files to modify/create

- `extension/src/windowRegistry.ts`
- `extension/src/sessionState.ts`
- `extension/src/autoReopen.ts`
- `extension/src/extension.ts`

## Background

The AYDY Companion extension reopens a single-folder window as its generated workspace file so the window title can carry the session count (`extension/src/extension.ts`: `autoReopen` `:296-317` on startup, `showCountInTabTitle` `:351-362` from the **AYDY: Show Session Count in Tab Title** command; both end in `openWorkspace` → `reopenAsWorkspace` → `openFolder(path, { forceReuseWindow: true })`, `reopenAsWorkspace` at `extension/src/autoReopen.ts:98-107`). The workspace file sits beside the folder (`siblingWorkspacePath`, `extension/src/workspaceFile.ts:55-56`) or, when the owner declined to overwrite a foreign file there, in AYDY's data dir (`legacyWorkspacePath`, `:62-70`).

Every window publishes `app.aydy/windows/<pid>.json` = `{ pid, folders }` (`extension/src/windowRegistry.ts`; `publish` in `extension.ts:412-414`, called on activation and on folder changes; type `WindowFolders` in `extension/src/sessionState.ts:13-16`). A folder window and a workspace window on the same folder publish the same `folders`, so today nothing can tell them apart.

Owner decision 8 (`.mise/goals.md`): when a window opened as a plain folder has its sibling `.code-workspace` already open in another VS Code window (known from the `app.aydy/windows` registry), AYDY tells the owner the folder is already open as a workspace in another window instead of calling `openFolder`. Found when this repo's folder window showed no title status while a second window had `aydy.code-workspace` open.

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `sessionState.ts`: `WindowFolders` gains `workspace?: string | null` (optional, so every existing literal — `knownWindows`' self entry, tests, `jumpRequests.ts` — still compiles).
2. `windowRegistry.ts`:
   - `publishWindow(pid, folders, workspace: string | null = null, options)` — keep `options` last; update the signature carefully (existing callers pass `{ dir }` as the third argument in tests: move those to the fourth or make the call sites explicit). The file body becomes `{ pid, folders, workspace }`.
   - `parseWindow` accepts a missing `workspace` (older extension versions) as `null`, a string as itself, anything else as `null` — never rejects the entry over it.
3. `autoReopen.ts`: `export const workspaceOpenElsewhere = (folder: string, selfPid: number, windows: readonly WindowFolders[], dirs?: WorkspaceDirs): string | null` — returns the `workspace` of the first window with `pid !== selfPid` whose `workspace` equals `siblingWorkspacePath(folder)` or `legacyWorkspacePath(folder, dirs)`; else `null`. Pure.
4. `extension.ts`:
   - `publish(folders)` passes the open workspace file: `openWorkspaceFile()` resolved with `realpathSync` (fall back to the raw path on error), so it compares equal to `siblingWorkspacePath(realpath(folder))`.
   - A helper `announceWorkspaceElsewhere(folder): Promise<boolean>` reads `readWindows()` (from `windowRegistry.ts`), calls `workspaceOpenElsewhere(folder, process.pid, windows)`, and when it finds one shows `vscode.window.showInformationMessage(`AYDY: ${basename(folder)} is already open as a workspace in another window (${tildePath(path)}). That window's tab shows the session count.`)` and returns `true`.
   - `autoReopen`: after `settle()` and the `openWorkspaceFile() !== undefined` check, `if (await announceWorkspaceElsewhere(folder)) return` — before `reasonsNotToReopen`, so no reopen prompt is shown either.
   - `showCountInTabTitle`: same check before `openWorkspace(folder)`.

## Gotchas

- `vscode.workspace.workspaceFile.fsPath` may not be a realpath (symlinked home folders); `localFolders()` already realpaths folders (`extension.ts:401-410`), so realpath the workspace too or the comparison silently never matches.
- Window files of dead pids are already cleaned by `readWindows`; no new cleanup is needed.
- Ownership of sessions (`ownerOf`) is not part of this fix; do not change it.
- This changes the window file shape other windows read: `parseWindow` must keep accepting files from older extension builds without `workspace`.

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests:
  - `windowRegistry.test.ts`: `publishWindow` writes `workspace`; `readWindows` reads it back; a file without `workspace` reads as `workspace: null`; a non-string `workspace` reads as `null` and the entry is kept.
  - `autoReopen.test.ts`: `workspaceOpenElsewhere` finds another window holding the sibling path; finds one holding the legacy path; ignores this window's own entry; ignores windows with another folder's workspace or none; returns `null` with no windows.
- Substitute check (extension host behaviour is not unit-tested here):
  Run it in an **isolated** VS Code instance. Never run `code --install-extension` without `--extensions-dir`, never open the owner's own folders, and never use the owner's real VS Code profile, `~/.claude` or `~/Library/Application Support/app.aydy`: the packaged build has the same version (0.1.0) as the owner's daily build, so installing it there would silently replace it, run in every open window, rewrite every sibling `.code-workspace` and run `installHookScript`/`ensureHooks` against the real home.
  1. Make `S`, a new directory under the session scratchpad (`mkdir -p "$S"`), and record the baseline: `code --list-extensions --show-versions > "$S/before.txt"` and `stat -f %m ~/.claude/settings.json`.
  2. Set up scratch dirs: `H="$S/home"`; `mkdir -p "$H/Library/Application Support/app.aydy" "$S/ext" "$S/data" "$S/folders/demo"`.
  3. `yarn package-extension`, then `env -u CLAUDE_CONFIG_DIR HOME="$H" code --extensions-dir "$S/ext" --install-extension extension/aydy-companion-0.1.0.vsix`.
  4. Launch: `env -u CLAUDE_CONFIG_DIR HOME="$H" code --user-data-dir "$S/data" --extensions-dir "$S/ext" --new-window "$S/folders/demo"` (a separate `--user-data-dir` starts a separate VS Code instance that inherits this environment).
  5. Guard: within a few seconds `"$H/Library/Application Support/app.aydy/windows/"` must hold a `<pid>.json` for the new window, and the real `~/Library/Application Support/app.aydy/windows/` must not gain that pid. If the window file appears under the real home instead, quit the isolated instance at once, skip the rest of the check and report it as not run.
  6. Let window A (launched in step 4 on `"$S/folders/demo"`) reopen itself as `"$S/folders/demo.code-workspace"` (auto-reopen) — or open that file in it — then open the plain folder again in a second window: `env -u CLAUDE_CONFIG_DIR HOME="$H" code --user-data-dir "$S/data" --extensions-dir "$S/ext" --new-window "$S/folders/demo"`.
  7. Window B shows `AYDY: demo is already open as a workspace in another window (…demo.code-workspace). That window's tab shows the session count.`, shows no reopen prompt, and focus stays on window B. Capture the screen with `screencapture -x "$S/shot.png"` and Read it to confirm; run **AYDY: Show Session Count in Tab Title** in window B and confirm the same message.
     Cleanup (always, even after a failure): quit the isolated instance only (close its windows, or `pkill -f -- "--user-data-dir $S/data"` — the pattern must contain the scratch path so no other VS Code process matches), confirm `code --list-extensions --show-versions` still equals `"$S/before.txt"` and `~/.claude/settings.json`'s mtime is unchanged, then `rm -rf "$S"`. Report any difference to the owner instead of trying to repair their profile.
