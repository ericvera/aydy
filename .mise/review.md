# Review

## Tasks

- 01_01 — Working glyph `◐` and repo-aware window titles (`repo · worktree`, `repo · branch`, `repo` on main), via new extension/src/repoLabel.ts, `windowGlance` in sessionState.ts, `onGlance` in windowTracker.ts, HEAD watcher in extension.ts; docs updated. Verify: `yarn test` (repoLabel, sessionState, windowTracker tests), then in a live window check the glyph goes ◐ while a prompt runs, ● on Stop/Notification, and the title follows a branch switch (not yet checked by hand).

## Open assumptions

- `◐` shows no count; `●` keeps `● N` / `● 9+`.
- "On main" also covers `master`; detached HEAD shows `<glyph> <repo>`.
- Non-git folders keep `<glyph> <basename>`.
- Repo name resolved like Rust's `identify` (repo.rs), ported to TS.
- Title refreshes on HEAD change, main checkouts only.
- No mock (window-title string, not Vue UI).

## Reviewer minors (not fixed)

- extension.ts:112 — first `onGlance` can write an old-format `○ <basename>` title before the label resolves, then rewrite it.
- extension.ts:141-146 — overlapping HEAD-watcher refreshes can leave an older branch in the title after fast back-to-back switches.

## Amendments

- None.
