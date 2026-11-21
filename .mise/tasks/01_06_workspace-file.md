# 01_06 Generated workspace file: build, settings sync, title write

## Goal

Add the code that creates and maintains AYDY's generated `.code-workspace` file for a folder. It copies the folder's `.vscode/settings.json` into the file and writes `window.title`. It never touches the repo.

## Files to modify/create

- extension/src/jsonc.ts (new, pure)
- extension/src/workspaceFile.ts (new)

## Background

- Why this exists: the only way to change a VS Code window tab's text is the `window.title` setting. There is no title API in the `window` namespace of the pinned `@types/vscode` 1.90.0, checked in `node_modules/@types/vscode/index.d.ts:10343-11100`. Writing it into the folder's `.vscode/settings.json` would dirty the repo, so a workspace file outside the repo carries it.
- In workspace-file mode VS Code does not apply window-scoped settings from the folder's `.vscode/settings.json`. So the generated file copies that file's settings.
- Task 01_01 produced `extension/src/aydyPaths.ts` with `workspacesDir()`. Task 01_02 produced `extension/src/atomicWrite.ts` with `writeAtomically`.
- File shape (`.mise/spec.md`, "Extension modules"):

```json
{
  "folders": [{ "path": "/abs/folder" }],
  "settings": { "<copied keys>": "...", "window.title": "○ okven" },
  "aydy": { "folder": "/abs/folder", "copiedSettings": ["<copied key>"] }
}
```

- Location: `<workspacesDir>/<first 12 hex of sha256(realpath folder)>/<basename(folder)>.code-workspace`.
- Nothing is bundled, and `vsce package --no-dependencies` ships no runtime npm packages, so JSONC parsing must be hand-written.

## Guides

None match.

## Implementation details

- `jsonc.ts`: `parseJsonc(text): unknown`. It strips `//` and `/* */` comments outside strings, removes trailing commas before `}` or `]`, and then calls `JSON.parse`. It throws on invalid input. Strings that contain `//` or escaped quotes must survive.
- `workspaceFile.ts`:
  - `generatedWorkspacePath(folderRealPath, dirs?)`.
  - `isGeneratedWorkspace(fsPath, dirs?)` is true when the path is under `workspacesDir`.
  - `buildWorkspace(existing: Record<string, unknown> | null, folder, folderSettings: Record<string, unknown>, title: string | null)` is pure:
    1. Start from `existing ?? {}`.
    2. Set `folders` to `[{path: folder}]`.
    3. In `settings`, delete the keys listed in `existing.aydy.copiedSettings`, then assign `folderSettings`.
    4. Set `window.title` when `title !== null`. Otherwise keep the existing value.
    5. Set `aydy = {folder, copiedSettings: Object.keys(folderSettings).filter(k => k !== 'window.title')}`.

    The `window.title` from folder settings is not copied, since AYDY owns it.
  - `readFolderSettings(folder)` reads `<folder>/.vscode/settings.json` with `parseJsonc`. A missing file gives `{}`. A parse failure gives `null`, which tells the caller to keep the previous copy.
  - `writeWorkspace(folder, title)` uses realpath and writes with `buildWorkspace(readExisting(), folder, readFolderSettings() ?? previousCopied, title)`. It creates dirs and returns the path.
  - `setWorkspaceTitle(path, title)` reads fresh, writes only when `settings['window.title'] !== title`, and returns whether it wrote.
  - `syncFolderSettings(path)` reads `aydy.folder` from the file and rewrites it with fresh folder settings, keeping the title.
  - The output is JSON with 2-space indent and a trailing newline.

## Gotchas

- The user may add settings through the VS Code Settings UI "Workspace" tab, which writes into this file. Always read fresh, and only remove keys AYDY itself copied.
- When the folder settings file fails to parse (mid-edit), keep the previously copied keys. Never wipe them.
- Hash the realpath, so `/tmp/x` and `/private/tmp/x` map to one file.

## Verification

- New `extension/src/jsonc.test.ts` covers line and block comments, a comment marker inside strings, escaped quotes, trailing commas, and that invalid input throws.
- New `extension/src/workspaceFile.test.ts` runs against temp dirs and covers:
  - path stability and realpath hashing
  - the generated file's shape
  - re-sync removes deleted keys, keeps user-added keys and keeps the title
  - `setWorkspaceTitle` skips writing when unchanged (mtime untouched)
  - a broken folder settings file keeps the prior copy
  - `isGeneratedWorkspace`
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
