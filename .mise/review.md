# Review

## Tasks

- 01_01 `src-tauri/src/session/signals.rs`: stale (Working + quiet ≥5m) and progress rule; ETA reports older than the turn are rejected. Verify: `cargo test signals`.
- 01_02 `transcript.rs::last_activity`: newest mtime of the transcript and subagent `.jsonl` files (≤3 path parts under `subagents/`). Verify: `cargo test last_activity`.
- 01_03 reconciler/registry: the 25 s scan feeds transcript activity into sessions; snapshots carry `signals`. Verify: `cargo test registry`.
- 01_04 `file_feed.rs`/`status_files.rs`: reads `app.aydy/progress/<id>.json` every 0.5 s; retries reports for sessions not yet known. Verify: `cargo test file_feed`.
- 01_05 payload + tray: `signals` reaches the popover; new Stale tray icon (dashed right half, `icons/tray/stale.png`), below unread, above Working. Verify: let a session sit quiet 5m and check the menu bar icon.
- 02_01 popover: Working rows show a line (`4/7 · ~6m left`, `Quiet 9m`, `3m elapsed`) with a small bar; stale rows get a hollow blue dot. Verify: open the popover during a long turn.
- 02_02 hover detail: `Quiet 9m · nothing written to the transcript`, `This turn 12m`, `ETA reported 1m ago` / `No ETA reported`. Verify: hover a Working row.
- 03_01 plugin: `.claude-plugin/marketplace.json`, `plugin/` with skill `aydy:progress` and `aydy-progress --eta/--step`; README has the mise line. Verify: `/plugin marketplace add <path-to-repo>`, `/plugin install aydy@aydy`, then ask an agent to report progress.
- 03_02 extension: same stale/progress rule in `extension/src/workSignal.ts`, reading the same files. Verify: `yarn test workSignal`.
- 03_03 window title: `◐ 4/7 · ~6m <label>`, `◐ 3m <label>`, `◌ quiet 9m <label>`; most urgent session wins (stale, then shortest ETA, then longest elapsed). Verify: reinstall the extension and watch this window's title during a turn.
- 03_04 folder window: when the folder's `.code-workspace` is open in another window, AYDY says so instead of reopening. Verify: open the folder alone while the workspace window is open. (Isolated VS Code check could not launch; unit tests only.)

## Known minor issues (not yet fixed)

1. `extension/src/workSignal.ts` `formatMinutes` rounds down: a fresh `--eta 6m` shows `~5m` in the title.
2. `extension/src/windowTracker.ts:278`: the new `await signalFor` runs after the `disposed` guard, so a title write can happen during deactivation.
3. `extension/README.md:48,133-143`: reopen bullets omit the new open-elsewhere exception and the `workspace` field.
4. `docs/design/menubar-ux.md:179-181`: "no row carries anything beyond its dot and title" contradicts the progress line.
5. `docs/design/menubar-ux.md:269-272`: window-title description predates the signal text and `◌`.

## Open assumptions

- Stale threshold fixed at 5 minutes; not configurable.
- Progress comes only from `aydy:progress`; without a report, elapsed time is shown. No TodoWrite fallback.
- An ETA reported before the current prompt is discarded.
- The plugin writes nothing when `app.aydy` doesn't exist; session id comes from `$CLAUDE_CODE_SESSION_ID`.
- Subagent transcript writes count as activity, so a session busy in subagents isn't stale.
- Stale tray icon ranks below the two unread states and above Working.
- A stale row with unknown quiet time reads just `Quiet`.

## Amendments

None.
