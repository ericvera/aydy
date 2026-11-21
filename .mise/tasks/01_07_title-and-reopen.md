# 01_07 Write the tab title, offer to reopen as a workspace

## Goal

Make the count visible. In a window opened from AYDY's generated workspace file, the extension writes `window.title` whenever the count changes, and keeps the copied folder settings in sync. In a plain-folder window, it offers once per folder to reopen from a generated workspace file. A palette command does the same later. Nothing reopens automatically.

## Files to modify/create

- extension/src/reopenOffer.ts (new)
- extension/src/extension.ts
- extension/package.json
- extension/README.md

## Background

- Task 01_05 wired a `WindowTracker` into `extension/src/extension.ts`. Its `onCount` stores the count in a module variable.
- Task 01_04 produced `tabTitle(count, name)` in `extension/src/sessionState.ts`.
- Task 01_06 produced, in `extension/src/workspaceFile.ts`: `generatedWorkspacePath`, `isGeneratedWorkspace`, `writeWorkspace(folder, title)`, `setWorkspaceTitle(path, title)` and `syncFolderSettings(path)`.
- Owner decision 2: reopening happens only on the owner's click.
- Mock (`.mise/mock/vscode-reopen-offer.html`, `.mise/mock/vscode-tab-count.html`):
  - State A, the offer: an information notification "Show how many Claude Code sessions need you in this window's tab? AYDY reopens this window from a workspace file kept outside the repo. This restarts the window, which may end Claude Code sessions open in it." with the buttons **Not now** and **Reopen as workspace**. It is shown once per folder on activation.
  - State B: the palette command `AYDY: Show Session Count in Tab Title`, listed only in a plain-folder window.
  - Plain-folder windows keep VS Code's default title.
  - Windows opened from the user's own `.code-workspace` are not covered. Write nothing there.
- `vscode.workspace.workspaceFile` (`index.d.ts:13080`) is the `Uri` of the open workspace file, or undefined. The context key `workbenchState` is `folder` for plain-folder windows.

## Guides

None match.

## Implementation details

- `reopenOffer.ts` exports pure helpers plus a thin action.
  - `offerKey(folder)` returns `aydy.reopenOffered:<realpath>`.
  - `shouldOffer(state: {workspaceFile?: string, folders: string[], offered: boolean})` is true only for exactly one folder, no workspace file, and not yet offered.
  - `reopenAsWorkspace(folder, title, env)` calls `writeWorkspace(folder, title)`, then `env.openFolder(path, {forceReuseWindow: true})`, where `env: {openFolder(path: string, options: {forceReuseWindow: boolean}): Thenable<unknown>}` takes the workspace file path as a plain string. `reopenOffer.ts` must not import `vscode` (not even `Uri`): `reopenOffer.test.ts` loads it under vitest, which has no `vscode` module, and the spec keeps the `vscode` API in `extension.ts` and the environment objects it passes in.
  - In `extension.ts`, `env.openFolder` is `(path, options) => vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(path), options)`. The `Uri` is built there.
- `extension.ts`. Let the window be generated when `workspaceFile?.scheme === 'file'` and `isGeneratedWorkspace(fsPath)`.
  - Generated window:
    - `onCount` calls `setWorkspaceTitle(path, tabTitle(count, basename(folder)))`, with the folder taken from the file's single folder.
    - On activation, call `syncFolderSettings(path)`.
    - Watch `<folder>/.vscode/settings.json` with `vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(folderUri, '.vscode/settings.json'))` and re-sync on create, change and delete.
  - Plain single-folder window: when `shouldOffer` is true, mark the folder offered in `context.globalState` first, then show the notification. **Reopen as workspace** calls `reopenAsWorkspace(folder, tabTitle(currentCount, name))`.
  - Register the command `aydy.showSessionCountInTabTitle`, which runs the same reopen without the once-guard. In a window that is not a plain single folder, it shows an info message saying it only applies to a window opened as a single folder.
- `package.json` `contributes`:

```json
"commands": [{ "command": "aydy.showSessionCountInTabTitle", "title": "Show Session Count in Tab Title", "category": "AYDY" }],
"menus": { "commandPalette": [{ "command": "aydy.showSessionCountInTabTitle", "when": "workbenchState == folder" }] }
```

  Add `onCommand` activation only if needed. `onStartupFinished` already covers it.
- `README.md`: document the tab count, the reopen offer and command, where the generated files live, and that reopening restarts the window.

## Gotchas

- Mark the folder offered **before** awaiting the notification, so a quick reload cannot show it twice.
- Write the title with `setWorkspaceTitle` only. Never through the configuration API, so there is a single writer.
- Folder names are written literally. VS Code still substitutes `${...}` inside the title. Acceptable, but note it in the README.

## Verification

- New `extension/src/reopenOffer.test.ts` covers the `shouldOffer` matrix (multi-root, workspace file present, already offered, no folders) and that `reopenAsWorkspace` writes the file (in a temp dir) before calling a fake `openFolder` with the written file's path string and `{forceReuseWindow: true}`.
- Run `yarn test`, `yarn build-tsc`, `yarn lint` and `yarn build-extension`. Do not run `yarn package-extension`: it writes over the owner's `extension/aydy-companion-0.1.0.vsix`.
- No real-environment check in this task. Installing the VSIX in the owner's VS Code would make the extension rewrite the real `~/.claude/settings.json` and write real seen and window files, which no implementer step may do (spec, "Verification boundary"). VS Code's handling of the generated file (the reopen, the `○`/`●` title, and whether a window-scoped `.vscode/settings.json` setting applies only through the copy) is checked by the owner at acceptance. In the README, state the window-scoped settings behaviour as unverified rather than as fact.
