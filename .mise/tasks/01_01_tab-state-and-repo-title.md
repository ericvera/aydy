# 01_01 Working glyph and repo-aware window titles

## Goal

The AYDY Companion VS Code extension (in `extension/`) writes each window's title
into a generated `.code-workspace` file as `<glyph> <name>`. Today the glyph is
`○` (nothing unread) or `●` / `● N` / `● 9+` (unread sessions), and the name is the
folder's basename. This task:

1. Adds a third glyph `◐` for "working": at least one of the window's sessions has a
   prompt running (its last hook event is `UserPromptSubmit`). Precedence:
   `●` beats `◐` beats `○`. `◐` carries no count; `●` keeps its count as today.
2. Replaces the basename with a repo-aware label, using `·` (U+00B7 middle dot) as
   the only separator:
   - Linked worktree: `<repo> · <worktree>` e.g. `● aydy · feat-tab-state`
   - Main checkout on a branch other than `main`/`master`: `<repo> · <branch>`
     e.g. `◐ aydy · feat/tab-state`
   - Main checkout on `main` or `master`, or detached HEAD: `<repo>`
   - Folder not in any git repo: the folder's basename (today's behaviour)
3. Refreshes the title when the main checkout's branch changes (HEAD watched), not
   only at window open.
4. Updates the two docs that describe the title format.

## Files to modify/create

- `extension/src/repoLabel.ts` (create) — TS port of Rust repo identification, HEAD branch reading, label building
- `extension/src/sessionState.ts` — `isWorking`, `WindowGlance`, `windowGlance`, new `tabTitle` signature
- `extension/src/windowTracker.ts` — emit a glance (unread + working) instead of a bare count
- `extension/src/extension.ts` — keep the glance and label, watch HEAD, pass both to `tabTitle`
- `extension/README.md` and `docs/design/menubar-ux.md` — format descriptions (docs, not code)

Tests: `extension/src/repoLabel.test.ts` (create), `extension/src/sessionState.test.ts`,
`extension/src/windowTracker.test.ts`.

## Background

- No prior tasks. Extension-only; the Rust app never reads window titles.
- Title glyph today: `tabTitle(count, name)` at `extension/src/sessionState.ts:121-129`.
  Status events: `SessionEvent` at `extension/src/sessionState.ts:1-2`. `isUnread`
  (`:62-67`) is true for `Stop`/`Notification` newer than the seen time;
  `unreadCount` at `:114-119`.
- `WindowTracker.pass` (`extension/src/windowTracker.ts:189-228`) collects the
  window's `owned` statuses, marks them seen while focused, then computes
  `unreadCount(owned, seen)` and calls `onCount(count)` only when it differs from
  `lastCount` (`:223-227`). Option type `WindowTrackerOptions.onCount` at `:44`.
- `extension/src/extension.ts`:
  - module-level `let unreadCount = 0` and exported `currentUnreadCount` (`:86-88`;
    `currentUnreadCount` has no callers anywhere — remove it).
  - `writeTitle` (`:102-113`) writes `tabTitle(unreadCount, basename(target.folder))`
    via `setWorkspaceTitle` (`extension/src/workspaceFile.ts:208-224`, which skips the
    write when the title is unchanged).
  - Other `tabTitle` callers, each with `basename(folder)`: `openWorkspace` (`:197-202`)
    and `migrateLegacy` (`:296-302`).
  - `keepGeneratedWorkspace` (`:124-150`) is where a file-system watcher pattern
    already exists (`createFileSystemWatcher(new RelativePattern(...))` pushed into
    `context.subscriptions`); follow it for the HEAD watcher.
  - The tracker is wired at `:455-463` (`onCount: (count) => { unreadCount = count; writeTitle() }`).
- Rust reference for repo identification: `identify` and helpers in
  `src-tauri/src/discovery/repo.rs:52-151`, with its tests at `:153-310`. Port the
  same rules:
  - Walk up from the folder through its ancestors. At the first directory holding
    `.git`: a directory means a normal checkout rooted there; a file means a pointer.
  - Pointer: take the first line whose trimmed text starts with `gitdir:`, trim the
    rest, resolve it against the directory holding the `.git` file, flattening `.`
    and `..` (`path.resolve` does this). Unreadable file or no `gitdir:` line → no repo.
  - If the resolved gitdir's parent is named `worktrees`, it is a linked worktree:
    worktree name = gitdir's last component; root = the parent of `worktrees`, and if
    that is named `.git` its parent instead (bare repos keep the `x.git` dir as root).
  - Any other pointer (e.g. a submodule's `.git/modules/x`) names the directory
    holding the `.git` file, with no worktree.
  - Repo name = root's last component with a trailing `.git` stripped unless that
    leaves it empty.
  - Non-existent folder or no `.git` above → no repo (not an error).
- Decisions from goals that reach this task: three glyphs with `●` > `◐` > `○`; `◐`
  shows no count; `◐` means `UserPromptSubmit` only (a `Notification` session is
  unread `●` until seen, then quiet `○`); "on main" means branch `main` or `master`;
  detached HEAD shows `<repo>`; not-in-git keeps `<basename>`; the
  `aydy.showSessionCountInTabTitle` command and setting names stay unchanged.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

### `extension/src/repoLabel.ts` (new)

Use `node:fs/promises` and `node:path` only (no `vscode` import, so it is unit
testable). Suggested shape:

```ts
export interface RepoIdentity {
  root: string          // main checkout root (or bare repo dir)
  name: string
  worktree: string | null
  gitDir: string        // directory whose HEAD names this checkout's branch:
                        // `<dir>/.git` for a .git directory, the resolved gitdir for a pointer
}
export const identifyRepo = async (folder: string): Promise<RepoIdentity | null>
export const readBranch = async (gitDir: string): Promise<string | null>
  // reads `<gitDir>/HEAD`; `ref: refs/heads/<branch>` → `<branch>` (may contain `/`);
  // anything else (detached sha) or a read error → null
export const repoLabel = (identity: RepoIdentity | null, branch: string | null, folder: string): string
  // pure: null identity → basename(folder); worktree → `${name} · ${worktree}`;
  // branch null/'main'/'master' → name; otherwise `${name} · ${branch}`
export interface FolderLabel { label: string; headPath: string | null }
export const folderLabel = async (folder: string): Promise<FolderLabel>
  // identifies, reads the branch only when there is no worktree, returns the label
  // and the HEAD file to watch (`<gitDir>/HEAD`) only for a non-worktree repo
  // (a worktree's label does not depend on its branch); never throws — any
  // unexpected error yields { label: basename(folder), headPath: null }
```

Write a short top-of-file comment pointing at `src-tauri/src/discovery/repo.rs` as the
rules this mirrors. Use `stat`/`lstat`-free checks via `fs.stat` (`isDirectory()` /
`isFile()`), treating ENOENT/ENOTDIR as "keep walking". Stop the walk at the
filesystem root (`dirname(dir) === dir`).

### `extension/src/sessionState.ts`

- `export const isWorking = (status: SessionStatus): boolean => status.event === 'UserPromptSubmit'`
- `export interface WindowGlance { unread: number; working: boolean }`
- `export const windowGlance = (statuses, seen): WindowGlance` → `{ unread: unreadCount(statuses, seen), working: statuses.some(isWorking) }`
- Change `tabTitle` to `tabTitle(glance: WindowGlance, label: string)`:
  unread > 0 → `● ${label}` / `● N ${label}` / `● 9+ ${label}` exactly as today;
  else working → `◐ ${label}`; else `○ ${label}`.

### `extension/src/windowTracker.ts`

- Replace `onCount: (count: number) => void` with `onGlance: (glance: WindowGlance) => void`
  in `WindowTrackerOptions` and the class field.
- In `pass`, compute `windowGlance(owned, seen)`; replace `lastCount` with
  `lastGlance: WindowGlance | null` and fire only when `unread` or `working` changed.

### `extension/src/extension.ts`

- Replace `unreadCount` with `let glance: WindowGlance = { unread: 0, working: false }`
  and delete `currentUnreadCount`.
- Add `let label: string | undefined` (label of the generated workspace's folder).
  `writeTitle` writes `tabTitle(glance, label ?? basename(target.folder))`.
- Tracker wiring: `onGlance: (next) => { glance = next; writeTitle() }`.
- Add `refreshLabel(folder)`: `const next = await folderLabel(folder)`; set `label`,
  call `writeTitle()`; return `next.headPath`. Log errors via the existing `log`.
- In `keepGeneratedWorkspace`, after setting `generated`, await `refreshLabel(folder)`
  (or kick it off and chain) and, when it returns a `headPath`, create
  `vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(vscode.Uri.file(dirname(headPath)), 'HEAD'))`
  whose create/change/delete each call `refreshLabel(folder)` again; push watcher and
  listeners into `context.subscriptions` like the settings watcher. The HEAD watcher is
  set up once from the first resolution; a folder that later becomes a worktree or
  repo is out of scope.
- `openWorkspace(folder)` and `migrateLegacy` compute their initial title with
  `tabTitle(glance, (await folderLabel(folder)).label)`.

### Docs

- `extension/README.md:84-86`: describe the three glyphs (`○` none, `◐` a prompt is
  running, `●`/`● N`/`● 9+` unread) and the label forms (`okven · eric-attribution-2`
  for a worktree, `okven · feat/x` for a branch, `okven` on main/master or detached,
  the folder name outside git).
- `docs/design/menubar-ux.md:236`: the sentence "The VS Code tab count — `● N folder`
  in each window's title —" becomes the new format (e.g. `● N repo · branch`), and
  mention `◐` for working, matching the menubar's working ring.

## Gotchas

- `◐` is U+25D0; `·` is U+00B7 surrounded by single spaces. Do not use `•` or `-`.
- A branch name may contain `/` (`feat/tab-state`); take everything after
  `refs/heads/`, trimmed of the trailing newline.
- For a linked worktree the label uses the **worktree directory name** (last component
  of the gitdir), not its branch, and no HEAD watcher is needed.
- Git updates HEAD by writing `HEAD.lock` and renaming it over `HEAD`; watch the `HEAD`
  pattern in its directory (not the file inode with `fs.watch`) so renames are seen.
  VS Code's default `files.watcherExclude` excludes `.git/objects` and
  `.git/subtree-cache` only, not `HEAD`.
- `setWorkspaceTitle` skips unchanged titles, so the first run after upgrade rewrites
  each generated workspace file's `window.title` once — expected and reversible.
- `extension/tsconfig.json` extends `@tsconfig/strictest`: `noUncheckedIndexedAccess`,
  `exactOptionalPropertyTypes`, etc. apply.
- Fixture strings `'● okven'` / `'○ okven'` in `autoReopen.test.ts` and
  `workspaceFile.test.ts` are just titles passed through; leave them.
- `autoReopen.ts`'s `isActive`/`countActive` (working or waiting, for reopen safety)
  stay as they are; do not reuse them for the glyph (they include `Notification`).

## Verification

Run: `yarn build-tsc`, `yarn lint`, `yarn test` (repo root). Rust is untouched;
`cargo clippy` / `cargo test` from `src-tauri/` should stay green unchanged.

Tests this task writes or updates:

- `extension/src/repoLabel.test.ts` (new; temp dirs via `realpathSync(mkdtempSync(join(tmpdir(), 'aydy-repo-')))`
  as in `extension/src/workspaceFile.test.ts:38-40`), mirroring the Rust cases in
  `src-tauri/src/discovery/repo.rs:158-309`: linked worktree (name, root, worktree);
  subdirectory of a worktree; normal checkout; subdirectory of a checkout; outside any
  repo → null; non-existent folder → null; `.git` file with no pointer → null; relative
  pointer resolves to the repo; submodule-style pointer names the working tree; bare
  repo worktree strips `.git`. Plus `readBranch` (branch, branch with `/`, detached
  sha → null, missing HEAD → null), `repoLabel` table for all four forms including
  `master`, and `folderLabel` returning `headPath` for a checkout and `null` for a
  worktree and outside git.
- `extension/src/sessionState.test.ts`: replace the `tabTitle` table (`:175-183`) with
  cases covering `○`, `◐`, `●`, `● 2`, `● 9+`, and unread-plus-working → `●`; add
  `windowGlance` cases (`UserPromptSubmit` → working; `Notification`/`Stop` not working).
- `extension/src/windowTracker.test.ts`: switch `startTracker` (`:113-140`) to
  `onGlance`, keeping the existing `counts` assertions by pushing `glance.unread`, and
  add a test that a `UserPromptSubmit` session in the window yields
  `{ unread: 0, working: true }` and that the callback is not repeated for an unchanged
  glance.
- Manual (no test covers the VS Code wiring): with the extension built
  (`yarn build-extension`), open a generated workspace window on a main checkout,
  switch branch, and confirm the title updates to `<glyph> <repo> · <branch>`; switch
  back to main and confirm `<glyph> <repo>`.
