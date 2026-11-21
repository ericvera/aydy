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

- `extension/src/extension.ts:120-170` (`activate` at `:120`) creates `AydyClient` (port file, register, heartbeat, command server) and `FocusReporter`, and wires `onDidChangeTabs` and `onDidChangeTabGroups` to report focus. `windowFacts` (`:52-58`), `activeTabLabel` (`:102-106`) and `focusState` (`:108-111`) exist only for them.
- `deactivate` (`:171`) closes the client. Since 01_05 it also calls `unpublishWindow(process.pid)`, which must stay.
- `versionOf` (`extension/src/aydyClient.ts:137`) is used by `claudeCodeVersion` (`extension.ts:47`) and must survive. Move it, with its tests, into `revealSession.ts` and `revealSession.test.ts`.
- Since stage 1 and 02_06, the extension's real work is in:
  - `installTracking` (01_03)
  - `WindowTracker` (01_05)
  - title and reopen (01_07)
  - `JumpResponder` (02_06)
- `revealSession.ts:1-29` doc says "AYDY only ever addresses the window its process-tree join placed the session in, and the window checks that the command was addressed to it". That is now false. Only the window that owns the session by the most-specific-folder rule (`ownerOf` in `sessionState.ts`, over the windows published in `<data>/windows/`) claims the jump request, by unlinking it.
- `package.json` description: "Tells AYDY which Claude Code sessions belong to this VS Code window".
- The doc comment on `CLAUDE_CODE_EXTENSION` (`extension/src/extension.ts:10-16`) says the version "is reported to AYDY". After this task nothing reports it; the constant only feeds `claudeCodeVersion`, which `revealEnvironment` uses for the reveal's version check (`revealSession.ts`).

## Guides

None match.

## Implementation details

- `extension.ts`:
  - Remove the client, the reporter and their helpers and subscriptions.
  - `deactivate` keeps only the `unpublishWindow(process.pid)` call from 01_05.
  - Keep `revealEnvironment` and `isClaudeTab`, because the jump responder uses them.
  - Rewrite the `CLAUDE_CODE_EXTENSION` doc comment: Claude Code exports no API, so nothing calls it; its version is read only so the reveal can refuse to drive a Claude Code version this integration has not been checked against. Drop the sentence about reporting the version to AYDY.
- `revealSession.ts`: add `versionOf` and rewrite the guard bullets in the header doc for the request-file path.
- `package.json` description: "Tracks Claude Code sessions for AYDY and shows how many need you in each window's tab".
- `README.md`:
  - Remove the loopback, port file and registration text.
  - Describe the hook script, the files under `~/Library/Application Support/app.aydy/`, the tab count and the reopen command.
  - Say that uninstalling removes the hook entries.

## Gotchas

- Do not remove `revealSession`, `isClaudeTab` or `REVEAL_COMMAND`. Jump still uses them.
- `extension.ts`'s `onDidChangeWorkspaceFolders` handler (`:153` before stage 1) and `onDidChangeWindowState` subscription (`:159` before stage 1) are shared. Since 01_05 and 01_07 they also carry work that must stay:
  - `onDidChangeWorkspaceFolders`: the folder rebuild (`fs.realpathSync` over the workspace folders), `publishWindow(process.pid, folders)`, and `poke()` on the `WindowTracker` (01_05), plus any title or reopen wiring 01_07 hung on folder changes.
  - `onDidChangeWindowState`: `poke()` on the `WindowTracker` (01_05).
  - Also keep the `WindowTracker` itself, its `onCount` (which writes the title through `setWorkspaceTitle`, 01_07), the `.vscode/settings.json` watcher, the reopen offer and command, `installTracking` and the `JumpResponder`.

  Remove only the client and reporter parts: `started.update(windowFacts())` from the folder handler, `reportFocus` from the window-state subscription (keep the subscription if it still pokes the tracker), the `onDidChangeTabs`/`onDidChangeTabGroups` subscriptions that only call `reportFocus`, and the disposable that calls `focus.dispose()` and `started.close()`. Read the file as it is after 01_05, 01_07 and 02_06 before editing; the line numbers above are from before stage 1.
- Grep `extension/src` for `aydyClient`, `focusReporter`, `listener.json` and `aydyPortFilePath` afterwards, and `extension/src/extension.ts` for `reported to AYDY`; none may remain.

## Verification

- Move the `versionOf` tests into `extension/src/revealSession.test.ts`.
- `yarn test`, `yarn build-tsc` and `yarn lint`.
- Package check, which must not touch the owner's artifact. `extension/aydy-companion-0.1.0.vsix` is the owner's installable build (gitignored; `extension/README.md:16` tells them to install from it). Do not run `yarn package-extension` (it writes over that file) and do not delete any `.vsix` under `extension/`.
  1. `rm -rf extension/out` so stale compiled files (`out/aydyClient.js`, `out/focusReporter.js`) cannot ship, then `yarn build-extension`.
  2. `mkdir -p <scratchpad>/vsix && cd extension && npx vsce package --no-dependencies --allow-missing-repository --skip-license -o <scratchpad>/vsix/aydy-check.vsix`, where `<scratchpad>` is the session's scratch directory.
  3. `unzip -l <scratchpad>/vsix/aydy-check.vsix` must list `extension/hook/aydy-hook.sh` and `extension/out/uninstall.js`, and must not list `out/aydyClient.js` or `out/focusReporter.js`.
  4. Delete only `<scratchpad>/vsix/aydy-check.vsix`. Confirm `extension/aydy-companion-0.1.0.vsix` still exists with its original mtime.
