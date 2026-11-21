# 03_01 Remove listener-era UI, add the Session tracking row

## Goal

The popover loses everything that depended on the app's listener and hook installer: the hook settings section, the extension prompt, the stale-list notice and Reconnecting header, and the port-move notice. Settings gains a "Session tracking: Installed" row.

## Files to modify/create

- src/App.vue
- src/components/organisms/SettingsPane.vue
- src/components/molecules/PopoverHeader.vue
- src/composables/useSessions.ts
- src/types/SessionList.ts

## Files to delete

Delete each of these with its colocated `.test.ts`:

- src/components/organisms/FirstRun.vue
- src/components/organisms/HookChangePreview.vue
- src/components/organisms/SettingsUnreadable.vue
- src/components/organisms/HookSettings.vue
- src/components/organisms/ExtensionPrompt.vue
- src/components/molecules/PortMoveNotice.vue
- src/components/organisms/StaleListNotice.vue
- src/composables/useHookStatus.ts
- src/composables/useExtension.ts
- src/types/AddedHook.ts
- src/types/RemovedHook.ts
- src/types/HookPreview.ts
- src/types/HookStatus.ts
- src/types/HookFailure.ts
- src/types/ExtensionStatus.ts
- src/types/PortMove.ts
- src/types/StaleList.ts

## Background

- Task 02_09 moved the popover gate to `useTracking` (`src/composables/useTracking.ts`) and the `InstallExtension` screen. It also stopped rendering FirstRun and SettingsUnreadable.
- `App.vue` still:
  - uses `useHookStatus` for the SettingsPane props (`hook-status`, `hook-failure`, `hook-busy`, `@reinstall`, `@remove-hooks`)
  - uses `useExtension` (`extension-connected`, the prompt dismissal)
  - renders `PortMoveNotice` (`sessions.portMove`), `StaleListNotice` (`sessions.stale`), `ExtensionPrompt` (`offeringExtension`) and `PopoverHeader :stale`
- `SettingsPane.vue`:
  - the "Companion extension" section at lines 105-127 says Connected or Not connected
  - it embeds `HookSettings` at about line 130
  - its props include `hookStatus`, `hookFailure`, `hookBusy` and `extensionConnected`
- `PopoverHeader.vue` has a `stale` prop (lines 11 and 28) that shows Reconnecting.
- `useSessions.ts` `EMPTY_LIST` (lines 7-14) and `src/types/SessionList.ts` carry `portMove`, `stale` and `extensionConnected`. The Rust snapshot still sends them until 03_03. Extra JSON fields are harmless.
- Mock `.mise/mock/popover-settings.html`: under a section labelled "VS Code extension", a row "Session tracking" whose value is "Installed", with the note "The extension records every Claude Code session, whether or not AYDY is running. AYDY only reads what it records." There is no Hooks section. The pane is only reachable when tracking is installed.
- Mock `.mise/mock/popover-sessions.html`: the list has no Reconnecting header, no stale notice, no port-move notice and no extension line under it.
- `src/utils/failureReason.ts` stays, because `useSettings` uses it.

## Guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

- `App.vue`:
  - Remove the imports, refs, handlers and template branches for everything deleted.
  - `openSettings` reads the settings and tracking only.
  - The header is called without `:stale`.
- `SettingsPane.vue`:
  - Drop the hook and extension props and emits, and the `HookSettings` element.
  - Replace the Companion extension section with `<SectionLabel>VS Code extension</SectionLabel>` and a `SettingRow label="Session tracking"` whose control text is "Installed" and whose note is the mock copy.
- `PopoverHeader.vue`: remove the `stale` prop and its Reconnecting rendering.
- `useSessions.ts` and `SessionList.ts`: remove `portMove`, `stale` and `extensionConnected`, and update the doc comment.
- Delete the files listed above. Grep `src/` for any remaining import of them, including test helpers in `src/__test__/`.

## Gotchas

- `SettingsPane.test.ts`, `PopoverHeader.test.ts`, `App.test.ts` and `useSessions.test.ts` reference the removed props and fields. Update them, do not delete them.
- Do not touch `Session.isCurrent` or `SessionRow.vue` here. That is 03_02.

## Verification

- Update the tests:
  - `SettingsPane.test.ts` renders "Session tracking" and "Installed" and has no Hooks section.
  - `PopoverHeader.test.ts` drops the stale cases.
  - `App.test.ts` drops the prompt, stale and port-move cases.
  - `useSessions.test.ts` uses the new empty list.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`. Grep for `useHookStatus|useExtension|PortMove|StaleList|ExtensionPrompt|HookSettings` under `src/`, which must return nothing.
