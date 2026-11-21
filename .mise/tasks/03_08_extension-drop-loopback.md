# 03_08 Extension drops the loopback client and focus reporter

## Goal

The VS Code extension stops trying to register with the AYDY app over loopback HTTP and stops reporting focus to it. The app no longer listens. The extension now works only through files.

## Files to modify/create

- extension/src/extension.ts
- extension/src/revealSession.ts
- extension/package.json
- extension/README.md

## Files to delete

- extension/src/aydyClient.ts and aydyClient.test.ts
- extension/src/focusReporter.ts and focusReporter.test.ts

## Background

- `extension/src/extension.ts:128-182` creates `AydyClient` (port file, register, heartbeat, command server) and `FocusReporter`, and wires `onDidChangeTabs` and `onDidChangeTabGroups` to report focus. `windowFacts`, `focusState` and `activeTabLabel` (`:60-66`, `:102-119`) exist only for them.
- `deactivate` (`:179-182`) closes the client.
- `versionOf` (`extension/src/aydyClient.ts:319-327`) is used by `claudeCodeVersion` (`extension.ts:55-56`) and must survive. Move it, with its tests, into `revealSession.ts` and `revealSession.test.ts`.
- Since stage 1 and 02_06, the extension's real work is in:
  - `installTracking` (01_03)
  - `WindowTracker` (01_05)
  - title and reopen (01_07)
  - `JumpResponder` (02_06)
- `revealSession.ts:1-29` doc says "AYDY only ever addresses the window its process-tree join placed the session in, and the window checks that the command was addressed to it". That is now false. The window that owns the session's folder claims the jump request by unlinking it.
- `package.json` description: "Tells AYDY which Claude Code sessions belong to this VS Code window".

## Guides

None match.

## Implementation details

- `extension.ts`:
  - Remove the client, the reporter and their helpers and subscriptions.
  - `deactivate` becomes a no-op, or is removed if VS Code does not need it.
  - Keep `revealEnvironment` and `isClaudeTab`, because the jump responder uses them.
- `revealSession.ts`: add `versionOf` and rewrite the guard bullets in the header doc for the request-file path.
- `package.json` description: "Tracks Claude Code sessions for AYDY and shows how many need you in each window's tab".
- `README.md`:
  - Remove the loopback, port file and registration text.
  - Describe the hook script, the files under `~/Library/Application Support/app.aydy/`, the tab count and the reopen command.
  - Say that uninstalling removes the hook entries.

## Gotchas

- Do not remove `revealSession`, `isClaudeTab` or `REVEAL_COMMAND`. Jump still uses them.
- Grep `extension/src` for `aydyClient`, `focusReporter`, `listener.json` and `aydyPortFilePath` afterwards.

## Verification

- Move the `versionOf` tests into `extension/src/revealSession.test.ts`.
- `yarn test`, `yarn build-tsc`, `yarn lint` and `yarn package-extension`. The VSIX must contain `hook/aydy-hook.sh` and `out/uninstall.js` (check with `unzip -l extension/*.vsix`, then delete the `.vsix`).
