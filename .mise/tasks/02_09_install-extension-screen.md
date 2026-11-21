# 02_09 Install-the-extension screen and popover gate

## Goal

When session tracking is not installed, the popover shows the "Install the AYDY extension in VS Code" screen (or "AYDY's hooks are missing") instead of the list. This replaces the hook-installing Setup screen, and the screen clears by itself once the files appear. The list now depends on tracking status, not on the app's own hooks.

## Files to modify/create

- src/types/TrackingStatus.ts (new)
- src/composables/useTracking.ts (new)
- src/components/organisms/InstallExtension.vue (new)
- src/App.vue
- docs/design/menubar-ux.md

## Background

- Task 02_08 added the Tauri command `tracking_status`. It returns `{ state: 'installed' | 'notInstalled' | 'hooksMissing' }`.
- `src/App.vue` today:
  - It routes `FirstRun` when `status !== null && !status.installed` (the hook status from `useHookStatus`).
  - The list and settings branches are gated on `status !== null`.
  - `SettingsUnreadable` shows when the hook status read failed (`App.vue:150-234`).
  - `onMounted` calls `subscribe()` and `refresh()`.
- After this task, `useHookStatus` stays in use **only** for the Settings pane's hook section. Task 03_01 removes it.
- Composable pattern: module-scoped refs plus `invoke` (`src/composables/useExtension.ts`, `src/composables/useSettings.ts`), with tests mocking `@tauri-apps/api/core` (`src/composables/useExtension.test.ts`).
- Screen pattern: `src/components/organisms/FirstRun.vue` (ScreenHeader titled "Setup", no gear, ActionButton, Banner). Its test is `FirstRun.test.ts`, with helpers in `src/__test__/`.
- Mock `.mise/mock/popover-install-extension.html`:
  - **A** (notInstalled): header "Setup", heading "Install the AYDY extension in VS Code", body "The extension tracks your Claude Code sessions, even while this app is closed. AYDY shows what it records.", a button **Show me how**, and a note "Already installed? Open any VS Code window and this screen goes away."
  - **B**: after Show me how, these steps appear inline:
    1. In VS Code, open the Command Palette.
    2. Run **Extensions: Install from VSIX…**
    3. Choose `aydy-companion.vsix`.
    4. Open any project window.
  - **C** (hooksMissing): heading "AYDY's hooks are missing", body "The extension is installed, but its hooks are not in `~/.claude/settings.json`, so sessions are not being recorded. Open any VS Code window and the extension puts them back."
- The doc section to update is `docs/design/menubar-ux.md:336-385`, "Setup: the screen before there is a list".

## Guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

- `TrackingStatus.ts`: `export type TrackingState = 'installed' | 'notInstalled' | 'hooksMissing'` and `export interface TrackingStatus { state: TrackingState }`.
- `useTracking.ts`, with module-scoped `status: Ref<TrackingStatus | null>`:
  - `read()` invokes `tracking_status`. A failure leaves the previous value and records nothing, because the app cannot do anything about it.
  - `watch()` reads now, then every 2000 ms while the state is not `installed`, and stops polling once it is installed.
  - `stop()`.
- `InstallExtension.vue`:
  - Prop `state: 'notInstalled' | 'hooksMissing'`.
  - The A, B and C copy exactly as in the mock, with a local `showingSteps` ref.
  - Uses the existing atoms and molecules (ScreenHeader, ActionButton, FieldNote) and has no gear.
- `App.vue`:
  - Call `useTracking().watch()` on mount.
  - Route `InstallExtension` when `tracking.status !== null && tracking.status.state !== 'installed'`.
  - Gate the list and settings branches on `tracking.status?.state === 'installed'` instead of the hook status.
  - Remove the `FirstRun` and `SettingsUnreadable` branches and their imports and handlers (`install`, `plan`, `revealSettingsFile`). Keep the rest of `useHookStatus` only as the SettingsPane props need it. When `status` is still null, pass the props the pane accepts as optional, or leave the pane branch gated on both.
  - Also re-read tracking when settings open.
- `menubar-ux.md`: rewrite the Setup section for the install screen and its three states. The app installs nothing, and the screen clears itself.

## Gotchas

- Do not delete `FirstRun.vue`, `SettingsUnreadable.vue` or their tests here. Task 03_01 deletes them along with the other dead components. Only stop rendering them.
- While tracking is `null` (not yet read), render nothing, as today. Guessing would flash the wrong screen.
- Follow `docs/design/ui-conventions.md` spacing and typography. Unread styling rules do not apply here.

## Verification

- New `src/composables/useTracking.test.ts`: `read` sets the status, polling continues while not installed and stops once installed (fake timers), and a failed invoke keeps the last value.
- New `src/components/organisms/InstallExtension.test.ts`: state A copy and the Show me how reveal, the B steps, and the state C copy.
- Update `src/App.test.ts`: notInstalled shows the install screen and no list, installed shows the list, and the FirstRun cases are removed.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
