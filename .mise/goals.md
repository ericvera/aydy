the title format is a bit writs. showing something like the circle followed by time then divider. Show me a fes suggestions to address this.

## Investigation

Design ask, not a bug — nothing to reproduce.

- Title assembly: `tabTitle` in extension/src/sessionState.ts:175-188 joins `glyph signal.text label` with spaces → `◐ 3m okven · feat/x`.
- Signal text: `titleSignal` in extension/src/workSignal.ts:173-193 builds `4/7 · ~6m`, reusing the label's `·` → `◐ 4/7 · ~6m okven · feat/x` (two identical dividers, time glued to repo). `etaText` (168-171) → `~6m` / `2m over`; line 175 → `quiet 9m`; `formatMinutes` (155-166) → `3m`, `<1m`, `1h 10m`.
- Label divider: `repoLabel` in extension/src/repoLabel.ts:144-157 writes `name · worktree` / `name · branch`; mirrors src-tauri/src/discovery/repo.rs.
- Popover wording differs: src/components/molecules/SessionProgressLine.vue:19-39 uses `~6m left`, `2m past ETA`, `3m elapsed`, `Quiet 9m`.
- Call sites take one string: extension/src/extension.ts:113, 242, 361; signal chosen in extension/src/windowTracker.ts:345.
- Tests: extension/src/sessionState.test.ts:208-223, extension/src/workSignal.test.ts:323-355; autoReopen.test.ts:201 and workspaceFile.test.ts:102,130,205,238 use only `● okven` / `○ okven`.
- Docs to update: extension/README.md:90-110, docs/design/menubar-ux.md:267-272.
- Titles are persisted to the user's `.code-workspace` via `setWorkspaceTitle` (extension/src/workspaceFile.ts:216-235); old-format titles linger until next write. No schema/API involved.
- Title fully replaces VS Code's `window.title`.

## Decisions

- Layout: repo label first, status last in square brackets; glyph stays in front. `◐ okven · feat/x [3m]`, `◐ okven · feat/x [1h 10m]`, `◐ okven · feat/x [<1m]`, `◐ okven · feat/x [~6m]`, `◐ okven · feat/x [4/7 · ~6m]`, `◐ okven · feat/x [4/7 · 2m over]`, `◌ okven · feat/x [quiet 9m]`.
- Status wording stays terse as today (`3m`, `~6m`, `2m over`, `quiet 9m`); the `·` between step and time stays (same character as the label's divider).
- Titles without a signal are unchanged: `● okven`, `● 3 okven`, `○ okven`, `◐ okven` (no brackets).

## Assumptions

- Popover wording (SessionProgressLine.vue) is out of scope and unchanged.
- No mock: the change is the VS Code window title string, not the app's Vue UI, and the owner approved the exact strings above.
- extension/README.md and docs/design/menubar-ux.md title examples are updated to the new format; tests asserting titles are updated.
- Old-format titles already persisted in `.code-workspace` files are left alone; they are replaced on the next title write.

## Proposal

- Issue: the window title puts status before the repo and reuses the label's `·`, so time runs into the repo name (`◐ 4/7 · ~6m okven · feat/x`).
- Fix: `tabTitle` puts the label first and the signal last in square brackets (`◐ okven · feat/x [4/7 · ~6m]`); update tests and the README/menubar-ux title examples.
- Skips: spec and critic — one small extension change, no schema or API change.
