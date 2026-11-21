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
  "aydy": {
    "folder": "/abs/folder",
    "copiedSettings": { "<copied key>": "<value AYDY last copied>" }
  }
}
```

- `folders[0]` is always the primary folder. The user may add more through "Add Folder to Workspace", and VS Code appends them to `folders`; they stay.
- `aydy.copiedSettings` records each copied key with the value AYDY copied, so a later sync can tell a key AYDY still owns (value unchanged) from one the user edited through the Settings UI (value differs).
- Location: `<workspacesDir>/<first 12 hex of sha256(realpath folder)>/<basename(folder)>.code-workspace`.
- Nothing is bundled, and `vsce package --no-dependencies` ships no runtime npm packages, so JSONC parsing must be hand-written.

## Guides

None match.

## Implementation details

- `jsonc.ts`: `parseJsonc(text): unknown`. It strips `//` and `/* */` comments outside strings, removes trailing commas before `}` or `]`, and then calls `JSON.parse`. It throws on invalid input. Strings that contain `//` or escaped quotes must survive.
- `workspaceFile.ts`:
  - `type WorkspaceDirs = {workspaces: string}`. Every function below that resolves a location takes `dirs?: WorkspaceDirs`, defaulting to `{workspaces: workspacesDir()}`. Only `extension.ts` relies on the default.
  - `generatedWorkspacePath(folderRealPath, dirs?)`.
  - `isGeneratedWorkspace(fsPath, dirs?)` is true when the path is under `dirs.workspaces`.
  - `buildWorkspace(existing: Record<string, unknown> | null, folder, folderSettings: Record<string, unknown>, title: string | null)` is pure:
    1. Start from `existing ?? {}`.
    2. `folders`: the primary entry first, then every existing entry (object with a string `path`) whose `path !== folder`, in their existing order. The primary entry is the existing entry whose `path === folder` when there is one (it keeps a user-set `name`), else `{path: folder}`. Entries that are not objects with a string `path` are dropped.
    3. `settings` (an object; anything else becomes `{}`). Let `prev` be `existing.aydy.copiedSettings` when it is a plain object, else `{}`.
       - For each key in `prev`: when `jsonEqual(settings[key], prev[key])`, AYDY still owns it, so delete it. Otherwise the user edited it; leave it.
       - For each `[key, value]` of `folderSettings` except `window.title`: when `key in settings` (after the deletes), the user owns it; leave it and do not record it. Otherwise set `settings[key] = value` and record `copied[key] = value`.
    4. Set `window.title` when `title !== null`. Otherwise keep the existing value.
    5. Set `aydy = {folder, copiedSettings: copied}`.

    The `window.title` from folder settings is not copied, since AYDY owns it.

  - `jsonEqual(a, b)` is a structural equality over JSON values (objects compared by key set, not key order).
  - `readFolderSettings(folder)` reads `<folder>/.vscode/settings.json` with `parseJsonc`. A missing file gives `{}`. A parse failure gives `null`, which tells the caller to keep the previous copy.
  - `writeWorkspace(folder, title, dirs?)` uses realpath and writes `generatedWorkspacePath(real, dirs)` with `buildWorkspace(readExisting(), folder, readFolderSettings() ?? previousCopied, title)`, where `previousCopied` is the existing `aydy.copiedSettings` object. It creates the directories under `dirs.workspaces` and returns the path.
  - `setWorkspaceTitle(path, title)` reads fresh, writes only when `settings['window.title'] !== title`, and returns whether it wrote.
  - `readWorkspaceFolder(path): Promise<string | null>` reads the generated file at `path` with `parseJsonc` and returns `aydy.folder` when it is a string, else `null` (missing file, parse failure, or no `aydy.folder`). It is exported; task 01_07 uses it to find a generated window's primary folder, so the file is parsed in one place.
  - `syncFolderSettings(path)` gets the folder through `readWorkspaceFolder(path)` (doing nothing when it is `null`) and rewrites the file with fresh folder settings, keeping the title.
  - The output is JSON with 2-space indent and a trailing newline.

## Gotchas

- The user may add or edit settings through the VS Code Settings UI "Workspace" tab, and add folders through "Add Folder to Workspace", both of which write into this file. Always read fresh. Only remove or overwrite a key whose current value still equals what AYDY last copied, and never drop a folder entry.
- A user who deletes a copied key in the Settings UI gets it back on the next sync while the folder file still has it (the folder file wins for keys nobody else holds). Acceptable.
- Every test passes `dirs` pointing into a fresh `fs.mkdtemp` directory. The default resolves to the real `~/Library/Application Support/app.aydy/workspaces/`, which no test may create or write (spec, "Verification boundary").
- When the folder settings file fails to parse (mid-edit), keep the previously copied keys. Never wipe them.
- Hash the realpath, so `/tmp/x` and `/private/tmp/x` map to one file.

## Verification

- New `extension/src/jsonc.test.ts` covers line and block comments, a comment marker inside strings, escaped quotes, trailing commas, and that invalid input throws.
- New `extension/src/workspaceFile.test.ts` runs against temp dirs (every `writeWorkspace`, `generatedWorkspacePath` and `isGeneratedWorkspace` call passes a temp `dirs`) and covers:
  - path stability and realpath hashing
  - the generated file's shape, including `aydy.copiedSettings` holding the copied values
  - re-sync removes deleted keys, keeps user-added keys and keeps the title
  - re-sync keeps folders the user added: an existing `folders` of `[{path: folder}, {path: '/other'}]` comes back with both, primary first
  - re-sync keeps a user edit: a copied key whose value in the file differs from `aydy.copiedSettings` keeps the user's value when the folder file changes that key or drops it, and is no longer listed in `copiedSettings`; an unedited copied key takes the folder file's new value
  - `jsonEqual` ignores key order
  - `setWorkspaceTitle` skips writing when unchanged (mtime untouched)
  - a broken folder settings file keeps the prior copy
  - `isGeneratedWorkspace`
  - `readWorkspaceFolder` returns `aydy.folder` from a file written by `writeWorkspace`, and `null` for a missing file, unparseable text, and a file without `aydy.folder`
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
