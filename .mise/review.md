# Review

## Tasks

- 01_01 (0c56c97): `tabTitle` (extension/src/sessionState.ts) returns `glyph label [signal]` instead of `glyph signal label`; tests in sessionState.test.ts and the README title section updated. Verify: run the extension with a working session — title reads `◐ okven · feat/x [3m]`, with steps `[4/7 · ~6m]`, stale `◌ … [quiet 9m]`; unread/idle titles unchanged.

## Open assumptions

- Popover wording (SessionProgressLine.vue) unchanged.
- No mock: window title string only; owner approved exact strings.
- docs/design/menubar-ux.md left unchanged: its title example has no signal.
- Old-format titles in `.code-workspace` files are replaced on the next title write.

## Open findings

None.

## Amendments

None.
