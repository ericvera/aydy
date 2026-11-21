# 01_03 Install hooks on activation, failure notice, uninstall cleanup

## Goal

On every activation the extension refreshes the hook script and makes sure the hook entries exist. If the settings file cannot be edited, exactly one window says so. Uninstalling the extension removes the entries.

## Files to modify/create

- extension/src/extension.ts
- extension/src/hookNotice.ts (new)
- extension/src/uninstall.ts (new)
- extension/package.json

## Background

- Task 01_02 produced `extension/src/hookInstall.ts`:
  - `installHookScript(extensionPath)`.
  - `ensureHooks()`, which returns `{ok:true, changed} | {ok:false, reason}`.
  - `removeHooks()`.
- Task 01_01 produced `extension/src/aydyPaths.ts` (`noticesDir()` and `claudeSettingsPath()`).
- Activation entry: `extension/src/extension.ts:120` (`activate`). The existing `AydyClient` and `FocusReporter` wiring stays untouched until task 03_08, because the app still uses the loopback channel in this stage.
- `extension/package.json` has `"activationEvents": ["onStartupFinished"]` and `"contributes": {}`. The VSIX is built by `yarn package-extension` (`package.json` scripts) from `out/`, compiled by `tsc -p extension/tsconfig.build.json`. Runtime npm dependencies are not shipped.
- Mock (`.mise/mock/vscode-reopen-offer.html`, state D):
  - Installation is silent when it works.
  - On failure, one window shows the error notification "AYDY could not add its hooks to ~/.claude/settings.json, so sessions are not being tracked. The file is not valid JSON. Nothing in it was changed." with the buttons **Open file** and **Try again**.
  - Every other window stays quiet.

## Guides

None match.

## Implementation details

- `hookNotice.ts`, IO, with the dir injectable for tests:
  - `claimFailureNotice()` creates `<noticesDir>/hook-install-failed` with flag `'wx'` (after `mkdir -p`). It returns true only for the window that created it.
  - `releaseFailureNotice()` unlinks it, ignoring `ENOENT`.
  - `failureMessage(reason, settingsPathForDisplay)` is a pure function. Its copy per reason:
    - `malformed`: "The file is not valid JSON."
    - `unexpected`: "Its hooks section is not in the expected shape."
    - `unreadable`: "The file could not be read."
    - `unwritable`: "The file could not be written."
    - `changedOnDisk`: "It kept changing while AYDY was writing."
  - Every message is followed by "Nothing in it was changed."
  - Show the path with `~` in place of the home directory.
- `extension.ts`: add `installTracking(context)`, called from `activate` without awaiting.
  1. Run `installHookScript(context.extensionPath)`, then `ensureHooks()`.
  2. On ok, call `releaseFailureNotice()`.
  3. On failure, if `claimFailureNotice()`, show `vscode.window.showErrorMessage(message, 'Open file', 'Try again')`.
     - **Open file** opens `claudeSettingsPath()` with `vscode.window.showTextDocument`.
     - **Try again** releases the notice and reruns.
  4. Catch everything. Activation must never throw.
- `uninstall.ts`: a top-level script for Node with no `vscode` import. It calls `removeHooks()` and swallows errors.
- `package.json`: add `"scripts": {"vscode:uninstall": "node ./out/uninstall.js"}`. VS Code runs this once the extension is fully uninstalled.

## Gotchas

- `vscode:uninstall` runs in plain Node, outside the extension host, and only after VS Code restarts. Any `vscode` import in its import graph crashes it, so keep `hookInstall.ts`, `hookSettings.ts` and `aydyPaths.ts` free of `vscode`.
- The notice marker survives VS Code restarts. That is intended ("says so once"). Success in any window clears it.
- Do not touch the legacy `X-Aydy-Hook` entries. The app still uses them in this stage.

## Verification

- New `extension/src/hookNotice.test.ts` checks:
  - the first claim is true and the second false
  - release followed by a claim is true again
  - `failureMessage` copy for each reason
- `uninstall.ts` stays a one-line call into `removeHooks`, which 01_02 tests. As a substitute check, run `node extension/out/uninstall.js` after `yarn build-extension` as `env -u CLAUDE_CONFIG_DIR HOME=<temp dir> node extension/out/uninstall.js`, where the temp dir holds `.claude/settings.json` with AYDY entries, and confirm they are removed. Unsetting `CLAUDE_CONFIG_DIR` keeps it from reaching the owner's real settings file (spec, "Verification boundary").
- Run `yarn test`, `yarn build-tsc`, `yarn lint` and `yarn build-extension`.
