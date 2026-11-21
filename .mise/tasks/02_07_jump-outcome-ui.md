# 02_07 Popover handles an unanswered jump

## Goal

The popover understands the new jump outcomes. A jump that no VS Code window answered shows the warning banner "No VS Code window took the jump. The window that had this session may be closed." That replaces "VS Code did not come forward."

## Files to modify/create

- src/types/JumpOutcome.ts
- src/composables/useJump.ts
- src/components/molecules/JumpFailureNotice.vue
- docs/design/menubar-ux.md

## Background

- Task 02_05 changed the Rust `JumpOutcome` (`src-tauri/src/jump/mod.rs`) to `jumped | noWindow | unanswered | duplicated`, serialized camelCase. `raised` and `unconfirmed` no longer exist.
- `src/composables/useJump.ts`:
  - `JumpFailure = Extract<JumpOutcome, 'unconfirmed' | 'duplicated'>` (line 8)
  - a rejected invoke is treated as `'unconfirmed'` (line 51)
  - failure outcomes set `failure` (line 54)
  - `noWindow` keeps the popover open (line 62)
- `src/components/molecules/JumpFailureNotice.vue:17-39`: `duplicated` uses the error variant and the rest use the warning, with the copy "VS Code did not come forward. AYDY asked…".
- The mock is `.mise/mock/popover-sessions.html`, state B: a warning banner with **No VS Code window took the jump.** in bold, then "The window that had this session may be closed." and a dismiss ✕.
- The doc section to update is `docs/design/menubar-ux.md:287-323`, "What a jump does, and what it says when it cannot".

## Guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

- `JumpOutcome.ts`: `'jumped' | 'noWindow' | 'unanswered' | 'duplicated'`. Rewrite the doc comment for the request-file mechanism.
- `useJump.ts`: the failure type becomes `'unanswered' | 'duplicated'`, and a rejected invoke maps to `'unanswered'`.
- `JumpFailureNotice.vue`: the prop type follows. For the non-duplicated case, render `<strong class="font-3">No VS Code window took the jump.</strong> The window that had this session may be closed.` Keep the duplicated copy and the variants unchanged.
- `menubar-ux.md`: rewrite the jump section. The app writes a request file, the window that owns the session reveals the tab and names what to raise, the app raises it, and no answer within a few seconds gives the warning above. Drop the mentions of the loopback channel and the process-tree join.

## Gotchas

- Follow `docs/design/ui-conventions.md` for the classes. Do not invent spacing or colour tokens.

## Verification

- Update `src/composables/useJump.test.ts`: `unanswered` and `duplicated` set failure, `jumped` closes, `noWindow` returns false, and a rejection gives `unanswered`.
- Update `src/components/molecules/JumpFailureNotice.test.ts` so the new copy renders for `unanswered`.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
