# 03_04 Folder window whose workspace is open elsewhere

## Goal

When a VS Code window opened as a plain folder has that folder's AYDY `.code-workspace` already open in another window, the extension tells the owner so instead of calling `vscode.openFolder` — which cannot reuse this window and only focuses the other one.

## Files to modify/create

- `extension/src/windowRegistry.ts`
- `extension/src/sessionState.ts`
- `extension/src/autoReopen.ts`
- `extension/src/extension.ts`

## Background

The AYDY Companion extension reopens a single-folder window as its generated workspace file so the window title can carry the session count (`extension/src/extension.ts`: `autoReopen` `:313-334` on startup, `showCountInTabTitle` `:371-382` from the **AYDY: Show Session Count in Tab Title** command; both end in `openWorkspace` → `reopenAsWorkspace` → `openFolder(path, { forceReuseWindow: true })`, `extension/src/autoReopen.ts:116-126`). The workspace file sits beside the folder (`siblingWorkspacePath`, `extension/src/workspaceFile.ts:55-56`) or, when the owner declined to overwrite a foreign file there, in AYDY's data dir (`legacyWorkspacePath`, `:62-70`).

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

- `vscode.workspace.workspaceFile.fsPath` may not be a realpath (symlinked home folders); `localFolders()` already realpaths folders (`extension.ts:400-410`), so realpath the workspace too or the comparison silently never matches.
- Window files of dead pids are already cleaned by `readWindows`; no new cleanup is needed.
- Ownership of sessions (`ownerOf`) is not part of this fix; do not change it.
- This changes the window file shape other windows read: `parseWindow` must keep accepting files from older extension builds without `workspace`.

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests:
  - `windowRegistry.test.ts`: `publishWindow` writes `workspace`; `readWindows` reads it back; a file without `workspace` reads as `workspace: null`; a non-string `workspace` reads as `null` and the entry is kept.
  - `autoReopen.test.ts`: `workspaceOpenElsewhere` finds another window holding the sibling path; finds one holding the legacy path; ignores this window's own entry; ignores windows with another folder's workspace or none; returns `null` with no windows.
- Substitute check (extension host behaviour is not unit-tested here): with the packaged extension installed, open a folder's `.code-workspace` in one window and the plain folder in another; the folder window shows the information message and does not jump focus to the other window. Close the windows and remove any scratch folders afterwards.
