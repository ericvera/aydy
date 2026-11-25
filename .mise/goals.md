we use the circle (empty and filled) to show agent state in the VS Code tabs. Should we use a third character to show work in progress? also, I think we should do show the repo name on worktrees. Like right now it is hard to tell which code bade the different windows belong to at a glance.

## Investigation

Not a bug fix. Extension-only change; Rust never reads window titles or glyphs.

- Glyphs: `tabTitle(count, name)` at extension/src/sessionState.ts:121-129 gives `○ name` for 0, `● name` for 1, `● N name` / `● 9+ name` above. Takes only the unread count — no input for "working".
- Callers: extension/src/extension.ts:110 (writeTitle), :198 (autoReopen), :300 (showCountInTabTitle), each passing `basename(folder)`.
- Data flow: WindowTracker.pass (extension/src/windowTracker.ts:189-228) computes only `unreadCount(owned, seen)` and fires `onCount(count)` (:223-227), wired into module-level `unreadCount` in extension.ts. A working state needs a second value on this path.
- Existing working rule: `isActive` at extension/src/autoReopen.ts:20-21 treats `UserPromptSubmit` or `Notification` as working/waiting; `countActive` at :24-32. Status events at sessionState.ts:1-2.
- Pattern: menubar icon already has "Working: ring with left half filled", ranked below unread and above quiet (docs/design/menubar-ux.md:20-45); Rust uses the same ranking (src-tauri/src/session/status.rs:33-37). ◐ would match.
- Worktree repo name: Rust resolves repo + worktree name from the `.git` file's `gitdir:` line in `identify` (src-tauri/src/discovery/repo.rs:52-120). Extension has no equivalent — needs a TS port. Rust label convention: `okven · eric/attribution-2` (repo.rs:7).
- Title write: `window.title` in the generated sibling `.code-workspace` file, setWorkspaceTitle at extension/src/workspaceFile.ts:208-224. Skipped when unchanged, so a new format rewrites each open window's workspace file once (reversible).
- Tests: tabTitle table at extension/src/sessionState.test.ts:176-183; ○/● in workspaceFile.test.ts and autoReopen.test.ts are fixture strings only.
- Docs describing the format: extension/README.md:84-86, docs/design/menubar-ux.md:236.
- The "VS Code tabs" are actually window titles. No schema, data-deletion or public API change; `aydy.showSessionCountInTabTitle` command name stays.

## Decisions

- Three glyphs in the window title: `○` quiet, `◐` working (a prompt is running, `UserPromptSubmit`), `●` done or needs attention (unread, including a session waiting on a Notification). Precedence: `●` beats `◐` beats `○`.
- Title format by folder kind (one separator everywhere: `·` middle dot):
  - Linked worktree: `<glyph> <repo> · <worktree>`, e.g. `● aydy · feat-tab-state`.
  - Main checkout on a branch other than main: `<glyph> <repo> · <branch>`, e.g. `◐ aydy · feat/tab-state`.
  - Main checkout on main: `<glyph> <repo>`.

## Assumptions

- `◐` shows no count; the unread count on `●` (`● N`, `● 9+`) stays as today.
- "On main" means the checked-out branch is `main` or `master`; detached HEAD shows `<glyph> <repo>`.
- A folder that is not in a git repo keeps today's title: `<glyph> <basename>`.
- The repo name is the main repository's folder name, resolved like Rust's `identify` (src-tauri/src/discovery/repo.rs), ported to TS in the extension.
- The title refreshes when the branch changes (HEAD watched), not only at window open.
- No mock: the change is a window-title string in the VS Code extension, not the Vue app's UI.
- extension/README.md and docs/design/menubar-ux.md format descriptions are updated to match.

## Proposal

- Issue: window titles show only unread/quiet and the folder basename, so working sessions and which repo/branch a window belongs to aren't visible at a glance.
- Approach: add `◐` working state alongside `○`/`●` (● > ◐ > ○), and title windows `repo · worktree`, `repo · branch`, or `repo` on main, via a TS port of Rust's repo identification, refreshed on HEAD change.
- Skips: spec and critic (extension-only, fits one implementer, nothing hard to undo).
