# 01_01 Window title: label first, status last in brackets

## Goal

Change the VS Code window title the AYDY extension writes so the repo label comes right after the glyph and the work signal (elapsed time, ETA, step, quiet time) comes last in square brackets:

- before: `◐ 4/7 · ~6m okven · feat/x`
- after: `◐ okven · feat/x [4/7 · ~6m]`

Titles without a signal stay exactly as they are today.

## Files to modify/create

- `extension/src/sessionState.ts` (function `tabTitle`)
- `extension/README.md` (section "The session count in the tab title", lines ~90-110)

Tests (do not count toward the limit): `extension/src/sessionState.test.ts`.

## Background

- The title is built in `tabTitle` at `extension/src/sessionState.ts:175-188`. Today the signal branch returns `` `${signal.stale ? '◌' : '◐'} ${signal.text} ${label}` ``.
- `signal.text` comes from `titleSignal` in `extension/src/workSignal.ts:173-193` and is one of: `3m`, `<1m`, `1h 10m`, `~6m`, `2m over`, `4/7 · ~6m`, `4/7 · 2m over`, `quiet 9m`. That wording is approved as-is: do NOT change `workSignal.ts` or its tests.
- `label` comes from `repoLabel` (`extension/src/repoLabel.ts:144-157`), e.g. `okven`, `okven · feat/x`, `okven · worktree`. Unchanged.
- Callers (`extension/src/extension.ts:113, 242, 361`) pass the result straight through as one string; titles are persisted into `.code-workspace` files by `setWorkspaceTitle` (`extension/src/workspaceFile.ts:216-235`). No caller changes are needed. Old-format titles already stored in users' workspace files are left alone; the next title write replaces them.
- Owner-approved strings (exact):
  - `◐ okven · feat/x [3m]`, `◐ okven · feat/x [1h 10m]`, `◐ okven · feat/x [<1m]`
  - `◐ okven · feat/x [~6m]`, `◐ okven · feat/x [4/7 · ~6m]`, `◐ okven · feat/x [4/7 · 2m over]`
  - `◌ okven · feat/x [quiet 9m]`
  - No signal, unchanged: `● okven`, `● 3 okven`, `● 9+ okven`, `○ okven`, `◐ okven`.
- Unread still wins over the signal (an unread window shows `● label` / `● N label` with no brackets) — keep that ordering.
- Out of scope: popover wording in `src/components/molecules/SessionProgressLine.vue` and its description at `docs/design/menubar-ux.md:160` (`~6m left`, `2m past ETA`, ...). `docs/design/menubar-ux.md:267-272` only shows `● N repo · branch`, which does not change; leave that file untouched.

## Guides

None of the config's Skills & guides entries match (no Vue component or popover change).

## Implementation details

1. In `tabTitle`, change the signal branch to:
   `` return `${signal.stale ? '◌' : '◐'} ${label} [${signal.text}]` ``
   Nothing else in the function changes.
2. In `extension/src/sessionState.test.ts:217-224` (`tabTitle: signal %j with %i unread`), update the expected strings:
   - `{ stale: false, text: '4/7 · ~6m' }, 0` -> `'◐ label [4/7 · ~6m]'`
   - `{ stale: true, text: 'quiet 9m' }, 0` -> `'◌ label [quiet 9m]'`
   - the two unread rows (`'● label'`, `'● 2 label'`) stay.
     Add rows covering a plain-time signal and a branch-style label, e.g. `{ stale: false, text: '3m' }` with label `okven · feat/x` -> `'◐ okven · feat/x [3m]'`, and `{ stale: false, text: '4/7 · 2m over' }` -> `'◐ okven · feat/x [4/7 · 2m over]'` (a separate `it.each` or an extra label column is fine).
3. In `extension/README.md` (section "The session count in the tab title"):
   - Change "While a session is working, the title also says how it is going, right after the glyph:" to say it appears at the end of the title in square brackets.
   - Rewrite the examples: `◐ okven · feat/x [4/7 · ~6m]`; `◐ okven · feat/x [~6m]` for an ETA without a step, and `[3m over]`-style once the ETA has passed (keep the existing explanation text); `◐ okven · feat/x [3m]`; `◌ okven · feat/x [quiet 9m]`; `◐ okven · feat/x` unchanged.
   - Keep the rest of the prose (urgency ordering, once-a-minute updates) as is.

## Gotchas

- Keep a single space between label and `[`, and no space inside the brackets.
- Do not touch the `·` inside `signal.text`; the owner chose to keep it.
- `autoReopen.test.ts:201` and `workspaceFile.test.ts:102,130,205,238` use only `● okven` / `○ okven` and must keep passing unchanged.

## Verification

- Run `yarn test` (covers `extension/src/sessionState.test.ts`, `workSignal.test.ts`, `autoReopen.test.ts`, `workspaceFile.test.ts`); all green.
- Run Check: `yarn build-tsc`, `yarn lint`, and `cargo clippy` from `src-tauri/` (no Rust changes expected; must stay green).
- Tests this task writes/updates: the `tabTitle: signal ...` cases in `extension/src/sessionState.test.ts` with the new bracketed expectations, plus the added `okven · feat/x` cases.
- Substitute check for the README: `grep -n "okven · feat/x" extension/README.md` shows only the new bracketed form for signal examples (no `◐ 4/7 · ~6m okven` or `◌ quiet 9m okven` left).
