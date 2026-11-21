# Review — extension core and tab count

## Tasks (what changed · how to verify)
- 01_01 Hook script `extension/hook/aydy-hook.sh` writes `app.aydy/status/<id>.json` per event, removes it on SessionEnd · `extension/src/hookScript.test.ts`
- 01_02 Hook-entry transforms, symlink/mode-safe atomic writer, install/remove IO · `hookSettings`, `atomicWrite`, `hookInstall` tests
- 01_03 Install on activation, once-only failure notice, `vscode:uninstall` cleanup, `extensionKind: ["ui"]` · `hookNotice.test.ts`; uninstall against a temp HOME
- 01_04 Status/seen store, unread rule, owner-by-longest-folder via launch cwd, window registry · `sessionState`/`sessionStore`/`windowRegistry` tests
- 01_05 WindowTracker: per-window unread count, seen writes on focus · `windowTracker.test.ts`
- 01_06 Generated `.code-workspace` under `app.aydy/workspaces/`, copies folder settings, keeps user folders/edits · `workspaceFile.test.ts`
- 01_07 Tab title `● N folder` (9+), reopen offer + "AYDY: Show Session Count in Tab Title", serialized writes · `reopenOffer.test.ts`
- 02_01 `lock_recovering` moved to `src-tauri/src/sync.rs` · cargo test
- 02_02 Shared hook payload parsing; status/seen reader `discovery/status_files.rs` · its tests
- 02_03 Unread from attention vs seen time; click writes seen; unarmed sound notifier · registry/sound tests
- 02_04 File feed replaces hook posts; live-only; one-pass hold before sounds; arms after first load · `file_feed.rs` tests
- 02_05 Jump via request file + reply, launch-cwd target, late-reply sweep · `jump/mod.rs` tests
- 02_06 Extension JumpResponder: owner claims (rename), reveals, replies · `jumpRequests.test.ts`
- 02_07 Jump outcomes `noWindow|unanswered|duplicated`; "No VS Code window took the jump." · `useJump`/`JumpFailureNotice` tests
- 02_08 `tracking_status` command (installed / notInstalled / hooksMissing) · `tracking.rs` tests
- 02_09 Install the extension screen; tracking re-checked on focus · `InstallExtension`/`useTracking`/`App` tests
- 03_01 Listener-era popover UI removed; Settings "Session tracking: Installed" · `SettingsPane`/`App` tests
- 03_02 "You are here" removed; `menubar-ux.md` rewritten for files · `SessionRow` tests
- 03_03 Listener health, 7 IPC commands, NotReceiving icon removed · cargo test
- 03_04 Installer/stash/port file removed; startup legacy cleanup strips `X-Aydy-Hook` entries and deletes `removed-hooks.json` · `hooks/legacy.rs` tests
- 03_05 HTTP listener and extension module deleted; axum/ureq dropped · `cargo tree -i axum` empty
- 03_06 Window association and process-tree matching removed · cargo test
- 03_07 Tab-match/event leftovers and `is_current` removed · cargo test
- 03_08 Extension loopback client/focus reporter removed; README rewritten; VSIX ships hook script · scratch VSIX listing
- Fix 1 Root README calls the extension required; tracking poll stays stopped; jump claim by rename (two unlinks both succeed on macOS)

## Open minor findings (not fixed)
- `extension/src/windowTracker.test.ts` "passes never overlap…" fails ~2/20 full runs
- Stale "listener" comments: `session/mod.rs:4`, `session/event.rs:26`, `notify/sound.rs:519,714`, `ipc/commands.rs:121`, `discovery/reconciler.rs:95-96`
- Stale "by unlinking it": `extension/src/revealSession.ts:16-17`, `extension/README.md:117-118`
- VSIX contents list omits `hook/aydy-hook.sh`: `extension/README.md:157`, `extension/.vscodeignore:1-3`
- Unused export `currentUnreadCount` in `extension/src/extension.ts:69`
- `PopoverHeader.vue:36` comment over 80 chars, "their own" → "its own"
- `workspaceFile.ts:94` relative folder paths may duplicate the primary folder (unverified against VS Code)

## Open assumptions — check at acceptance (your real setup)
- No title API exists (confirmed for @types/vscode 1.90); workspace-file mode ignoring `.vscode/settings.json` window settings is unverified
- Session-file `cwd` (launch dir) does not move — inconclusive, no live session had changed dir
- Reopening a window may end its Claude sessions — unverified
- Install VSIX → hooks land beside old ones; tab shows count, clears on arrival, caps 9+
- Stage-3 app on launch strips old hook entries and deletes the stash — quit installed AYDY first
- Stashed afplay hooks are not restored (decision 4)

## Amendments
- None
