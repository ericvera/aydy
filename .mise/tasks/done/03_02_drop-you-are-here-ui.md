# 03_02 Remove "you are here" and update the UX doc

## Goal

Session rows no longer show the "you are here" mark, because the app no longer hears which window has focus. The menu bar UX doc describes the file-based app.

## Files to modify/create

- src/types/Session.ts
- src/components/molecules/SessionRow.vue
- docs/design/menubar-ux.md
- src/**test**/aSession.ts (test fixture; sets `isCurrent`)
- src/components/organisms/SessionList.test.ts (test; sets `isCurrent`)

## Background

- `Session.isCurrent` (`src/types/Session.ts`) is rendered at `src/components/molecules/SessionRow.vue:46-49`. The Rust payload still sends `isCurrent` until 03_03, and the extra field is harmless.
- The test builder is `src/__test__/aSession.ts`, which sets `isCurrent`.
- Mock `.mise/mock/popover-sessions.html` state A: "No row says you are here any more: the app no longer hears which window has focus." Mock `.mise/mock/index.html`: the menu bar icon loses its Not receiving state. Removed screens: the Setup and change preview, the Hooks settings section, the extension line under the list, the Reconnecting header and stale notice, the port-move notice, and you are here.
- Sections of `docs/design/menubar-ux.md` still describing removed behaviour:
  - icon states (`:15-46`, including Not receiving)
  - "What clears an unread" (`:232-286`)
  - "Offering the companion extension" (`:324-335`)
  - Settings and "Hook status: two facts, never one" (`:386-451`)
  - Notices (`:452-499`)
  - "Where the numbers come from" (`:500+`)

  Tasks 02_07 and 02_09 already rewrote the jump and Setup sections.

## Guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

- Remove `isCurrent` from `Session.ts`, its rendering and styling in `SessionRow.vue`, and from `aSession.ts`.
- In `menubar-ux.md`:
  - Drop the Not receiving icon state.
  - Rewrite "What clears an unread". A session is unread when its last Stop or Notification is newer than its last seen time. Seen is written by the VS Code window when it has focus, and by a click in the popover. The VS Code tab count uses the same rule.
  - Remove "Offering the companion extension".
  - Replace the Settings hook sections with the Session tracking row.
  - Remove the stale and port-move notices.
  - Make "Where the numbers come from" point at the status and seen files.
  - Keep the rest.

## Gotchas

- Keep unread styling as weight and brightness (`docs/design/ui-conventions.md:118`). Only the you-are-here mark goes.

## Verification

- Update `src/components/molecules/SessionRow.test.ts` to remove the you-are-here cases and assert no such text renders. Remove `isCurrent` from `src/__test__/aSession.ts` and from `src/components/organisms/SessionList.test.ts`. `grep -rn isCurrent src` must then return nothing.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
