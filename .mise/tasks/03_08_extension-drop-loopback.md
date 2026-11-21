# 03_08 Extension drops the loopback client and focus reporter

## Goal

The VS Code extension stops trying to register with the AYDY app over loopback HTTP and stops reporting focus to it. The app no longer listens. The extension now works only through files.

## Files to modify/create

- extension/src/extension.ts
- extension/src/revealSession.ts
- extension/package.json
- extension/README.md
- extension/src/revealSession.test.ts (test; receives the moved `versionOf` assertions)

## Files to delete

- extension/src/aydyClient.ts and aydyClient.test.ts
- extension/src/focusReporter.ts and focusReporter.test.ts

## Background

- `extension/src/extension.ts:120-170` (`activate` at `:120`) creates `AydyClient` (port file, register, heartbeat, command server) and `FocusReporter`, and wires `onDidChangeTabs` and `onDidChangeTabGroups` to report focus. `windowFacts` (`:52-58`), `activeTabLabel` (`:102-106`) and `focusState` (`:108-111`) exist only for them.
- `deactivate` (`:171`) closes the client. Since 01_05 it also calls `unpublishWindow(process.pid)`, which must stay.
- `versionOf` (`extension/src/aydyClient.ts:137`) is used by `claudeCodeVersion` (`extension.ts:47`) and must survive. Move it into `revealSession.ts`. Its tests move into `revealSession.test.ts`, but only the `versionOf` assertions: `expect(versionOf({ version: '2.1.238' })).toBe('2.1.238')` (`aydyClient.test.ts:702`) and the three `versionOf` expectations at `aydyClient.test.ts:710-712` (`undefined`, `{}`, `{ version: 42 }` give `null`). The rest of that second test (`aydy()`, `portFile`, `new AydyClient`, registration) goes with the deleted file, as does its doc comment about registering.
- Since stage 1 and 02_06, the extension's real work is in:
  - `installTracking` (01_03)
  - `WindowTracker` (01_05)
  - title and reopen (01_07)
  - `JumpResponder` (02_06)
- `revealSession.ts:1-29` doc says "AYDY only ever addresses the window its process-tree join placed the session in, and the window checks that the command was addressed to it". That is now false. Only the window that owns the session by the most-specific-folder rule (`ownerOf` in `sessionState.ts`, over the windows published in `<data>/windows/`) claims the jump request, by unlinking it.
- `package.json` description: "Tells AYDY which Claude Code sessions belong to this VS Code window".
- Other doc comments that describe the loopback design (line numbers from before stage 1; find each by its quoted text):
  - `extension/src/extension.ts:23-27`, the `CLAUDE_PANEL_VIEW_TYPE` doc: "reporting any tab's label invites a source file named like a session summary to clear an unread the user never saw". Nothing reports tab labels any more; the label is only matched locally by `revealSession` when answering a jump.
  - `extension/src/extension.ts:113-119`, the `activate` doc: "Starts reporting this window to AYDY. … AYDY publishes its port and this window finds it."
  - `extension/src/extension.ts:170`, the `deactivate` doc: "Tells AYDY this window has gone, rather than leaving it to time out."
  - `extension/src/aydyClient.ts:129-136`, the `versionOf` doc, which moves with `versionOf`: "a manifest that has changed must leave this window registered rather than failing it".
  - `extension/src/revealSession.ts:136-138`, the comment in the `environment.open` catch: "letting it escape would leave AYDY treating this window as unreachable and dropping its registration".
  - `extension/src/revealSession.test.ts:160-164`, the doc on "answers rather than throwing when the command itself fails": "Letting the rejection escape would have AYDY read it as a window it cannot reach and drop the registration."
- The doc comment on `CLAUDE_CODE_EXTENSION` (`extension/src/extension.ts:10-16`) says the version "is reported to AYDY". After this task nothing reports it; the constant only feeds `claudeCodeVersion`, which `revealEnvironment` uses for the reveal's version check (`revealSession.ts`).

## Guides

None match.

## Implementation details

- `extension.ts`:
  - Remove the client, the reporter and their helpers and subscriptions.
  - `deactivate` keeps only the `unpublishWindow(process.pid)` call from 01_05.
  - Keep `revealEnvironment` and `isClaudeTab`, because the jump responder uses them.
  - Rewrite the `CLAUDE_CODE_EXTENSION` doc comment: Claude Code exports no API, so nothing calls it; its version is read only so the reveal can refuse to drive a Claude Code version this integration has not been checked against. Drop the sentence about reporting the version to AYDY.
  - Rewrite the `CLAUDE_PANEL_VIEW_TYPE` doc: getting the view type wrong makes every tab look like no Claude tab, so a reveal answers `notPresent` (the safe direction), whereas matching any tab would let a source file named like a session summary be taken for a session's tab. Drop "reporting any tab's label" and "clear an unread".
  - Rewrite the `activate` doc: it installs tracking, publishes this window's folders, keeps the tab count and title, and answers jump requests, all through files under AYDY's data directory; it needs no running app and asks nothing.
  - Rewrite the `deactivate` doc: it withdraws this window's published folder file so other windows stop counting it as an owner.
- `revealSession.ts`:
  - Add `versionOf` and rewrite the guard bullets in the header doc for the request-file path.
  - Rewrite the moved `versionOf` doc's last clause: a manifest that has changed must leave the reveal refusing (`unexpectedVersion`) rather than throwing. Drop "registered".
  - Rewrite the `environment.open` catch comment (`:136-138`): a thrown command is answered as `failed`, because it may still have opened something, and the jump responder must always be able to write a reply. Drop "unreachable" and "registration".
- `revealSession.test.ts`: rewrite the doc at `:160-164` the same way (a rejection that escaped would leave the jump without a reply). Delete no test.
- `package.json` description: "Tracks Claude Code sessions for AYDY and shows how many need you in each window's tab".
- `README.md`:
  - Remove the loopback, port file and registration text.
  - Describe the hook script, the files under `~/Library/Application Support/app.aydy/`, the tab count and the reopen command.
  - Say that uninstalling removes the hook entries.
  - Rewrite or remove these stale passages, each of which describes the loopback design:
    - `extension/README.md:3`: "Tells **AYDY** which Claude Code sessions belong to this VS Code window." Replace with the new description (as in `package.json`).
    - `:5-8`: "Without this extension it still lists and groups them … which sessions are in which window, which tab is in front, and the ability to be sent to one." Rewrite: the extension feeds AYDY every session's state through hook-written files, keeps each window's tab count, and answers jumps.
    - `:34-37`: "AYDY publishes the port it is listening on and the extension reads it; if AYDY is not running … does nothing at all". Keep "nothing to configure, no settings"; replace the rest: the extension works whether or not AYDY is running, because it only reads and writes files.
    - The whole "What it reports" section, `:39-90` (heading `## What it reports` through the paragraph ending "by being clicked."): registration, heartbeat, the extension host pid, focus and active-tab reporting, `:70` "Both are also said again whenever AYDY takes a fresh registration of this window", and the title-matching paragraphs. Remove it, or replace it with a short "What it does" section: the hook script writes `status/<session>.json`; each window writes `seen/<session>.json` when the user is looking at a session's window and publishes `windows/<pid>.json`; the tab count and jump answers are described in their own sections.
    - The guard bullet `:109-111`: "**The command names this window's process id.** A window that closes frees the port it named …". Remove it: requests are files, there is no addressed command.
    - The whole "How the two find each other" section, `:123-139` (`listener.json`, the extension's own loopback port, `ping`, registration). Replace it with one short paragraph: the app writes `jump/<id>.json`, only the owning window claims it by unlinking it, and that ownership rule is what keeps a reveal from reaching a window that does not host the session.
    - `extension/README.md:77`: "The tab label goes over raw and AYDY matches it against the session titles it holds". The label now goes in the jump request file and the owning window's `revealSession` matches it against its own Claude tabs.
    - `:104`: "refusing is reported to AYDY rather than swallowed". The refusal is now written as the `reveal` field of the reply file.
    - `:107`: "**AYDY placed the session in this window**, through the process tree". Replace with the ownership rule: only the window whose folder most specifically contains the session's launch directory claims the request.
    - `:154-156`: "The version is reported to AYDY either way" and "A window with no Claude Code extension at all registers exactly the same way". Nothing reports the version and nothing registers; say the version is only checked locally, and a window without Claude Code still tracks and answers jumps, it simply reveals nothing.
    The line numbers are from before stage 1; 01_07 added text to this README, so find each passage by its quoted text.

## Gotchas

- Do not remove `revealSession`, `isClaudeTab` or `REVEAL_COMMAND`. Jump still uses them.
- `extension.ts`'s `onDidChangeWorkspaceFolders` handler (`:153` before stage 1) and `onDidChangeWindowState` subscription (`:159` before stage 1) are shared. Since 01_05 and 01_07 they also carry work that must stay:
  - `onDidChangeWorkspaceFolders`: the folder rebuild (`fs.realpathSync` over the workspace folders), `publishWindow(process.pid, folders)`, and `poke()` on the `WindowTracker` (01_05), plus any title or reopen wiring 01_07 hung on folder changes.
  - `onDidChangeWindowState`: `poke()` on the `WindowTracker` (01_05).
  - Also keep the `WindowTracker` itself, its `onCount` (which writes the title through `setWorkspaceTitle`, 01_07), the `.vscode/settings.json` watcher, the reopen offer and command, `installTracking` and the `JumpResponder`.

  Remove only the client and reporter parts: `started.update(windowFacts())` from the folder handler, `reportFocus` from the window-state subscription (keep the subscription if it still pokes the tracker), the `onDidChangeTabs`/`onDidChangeTabGroups` subscriptions that only call `reportFocus`, and the disposable that calls `focus.dispose()` and `started.close()`. Read the file as it is after 01_05, 01_07 and 02_06 before editing; the line numbers above are from before stage 1.
- Grep afterwards; none may remain:
  - `extension/src` for `aydyClient`, `focusReporter`, `listener.json` and `aydyPortFilePath`
  - `extension/src/extension.ts` for `reported to AYDY`
  - `extension/README.md` for `reported to AYDY`, `process tree`, `registers`, `matches it against the session titles` and `port file`
  - `grep -rnEi 'registration|heartbeat|report(s|ing)? .*AYDY|reporting' extension/src extension/README.md extension/package.json`
  - `grep -rnwi port extension/src extension/README.md extension/package.json` (whole word, so `import`, `report` and `Application Support` do not count)

## Verification

- Move the `versionOf` assertions (and only those) into `extension/src/revealSession.test.ts`, as described in Background.
- The two loopback-wording greps from Gotchas return nothing.
- `yarn test`, `yarn build-tsc` and `yarn lint`.
- Package check, which must not touch the owner's artifact. `extension/aydy-companion-0.1.0.vsix` is the owner's installable build (gitignored; `extension/README.md:16` tells them to install from it). Do not run `yarn package-extension` (it writes over that file) and do not delete any `.vsix` under `extension/`.
  0. Record the owner's VSIX mtime before anything else: `stat -f "%Sm" extension/aydy-companion-0.1.0.vsix` (note the output; if the file does not exist, note that instead).
  1. `rm -rf extension/out` so stale compiled files (`out/aydyClient.js`, `out/focusReporter.js`) cannot ship, then `yarn build-extension`.
  2. `mkdir -p <scratchpad>/vsix && cd extension && npx vsce package --no-dependencies --allow-missing-repository --skip-license -o <scratchpad>/vsix/aydy-check.vsix`, where `<scratchpad>` is the session's scratch directory.
  3. `unzip -l <scratchpad>/vsix/aydy-check.vsix` must list `extension/hook/aydy-hook.sh` and `extension/out/uninstall.js`, and must not list `out/aydyClient.js` or `out/focusReporter.js`.
  4. Delete only `<scratchpad>/vsix/aydy-check.vsix`. Run `stat -f "%Sm" extension/aydy-companion-0.1.0.vsix` again and confirm the output equals what step 0 recorded (or that the file is still absent if it was absent).
