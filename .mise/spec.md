# Spec: stale and progress signals

## What

1. **Stale.** A Working session whose transcript has been quiet for 5 minutes or more is flagged stale. Stale is a signal beside status, not a fifth status: the four statuses, their rank and the no-decay rule (`src-tauri/src/session/status.rs`) do not change. No hook events and no `~/.claude/settings.json` change.
2. **Progress.** Agents report their own ETA and optional step through a Claude Code plugin skill, `aydy:progress`, shipped from this repository's own plugin marketplace. Its command `aydy-progress --eta <duration> [--step N/M]` writes a progress file AYDY reads. With no report, AYDY shows the current turn's elapsed time. There is no TodoWrite checklist fallback (goals decision 10). A report made before the current turn started is discarded.
3. **Surfaces** (chosen by the owner from the mock; every task inlines the copy, glyphs and sizes it needs):
   - **A, popover.** Every Working row gains an always-visible second line: a progress bar plus `4/7 · ~6m left`, `~6m left`, `2m past ETA` or `3m elapsed`; a stale row leads with **Quiet 9m** and its dot becomes a hollow ring in working blue. The hover detail line gains the stale explanation, this turn's time and how long ago the ETA was reported (or that none was).
   - **B, VS Code window title.** `◐ 4/7 · ~6m aydy · branch`, `◐ ~6m …`, `◐ 3m …`, and `◌ quiet 9m …` when stale. Unread still wins (`● 1 …`). A window with several working sessions shows the most urgent: stale first, then the shortest ETA.
   - **D1, tray icon.** A new Stale state — a dashed ring with its left half filled — ranks below the two unread states and above Working, and its count is the number of stale sessions.
4. **Folder-window fix.** A window opened as a plain folder whose sibling `.code-workspace` is already open in another VS Code window tells the owner so, instead of calling `vscode.openFolder` (which cannot reuse the window and just focuses the other one).
5. **mise integration** is documentation only: the plugin README shows the Skills & guides line `- aydy:progress (skill): at the start of each task and after each step`.

Not shipping: variant C (Working now strip), D2, E1–E4, any `--doing` option, a configurable threshold, any TodoWrite or task-list parsing.

## Design

### The shared rule (Rust app and VS Code extension)

Both the app and the extension derive the signals from the same files by the same rule, the way both already derive unread (menubar-ux.md, "What clears an unread"). Neither depends on the other running.

- `STALE_AFTER` = 5 minutes, a constant in each (`src-tauri/src/session/signals.rs`, `extension/src/workSignal.ts`).
- **Activity** of a session = the newest of: the transcript's mtime; the mtime of every `*.jsonl` under `<transcript dir>/<session id>/subagents/` (recursive, at most 3 path parts below `subagents/`: `subagents/agent-x.jsonl` is 1 part and counts, `subagents/workflows/wf_1/agent-y.jsonl` is 3 parts and counts, `subagents/a/b/c/agent-z.jsonl` is 4 parts and is ignored — subagents and workflow agents write there, not to the main transcript, so a session busy in a subagent would otherwise read as quiet); the session's last hook event; the accepted progress report's time. No transcript found → activity unknown → never stale.
- **Quiet for** = now − activity. **Stale** = status is Working and quiet for ≥ `STALE_AFTER`.
- **Turn start** = when the open turn began. The first `UserPromptSubmit`, or `Stop` with subagents still running, while no turn is open sets it to that event's time; further such events and `Notification`s keep it; a `Stop` with no subagents running clears it. This is `SessionTiming` in Rust (`start_turn` keeps an open turn's start, `src-tauri/src/session/timing.rs:40-44`; `apply_hook`, `session.rs:111-144`); the extension carries the same state per session across its passes (`nextTurnStart`), because its status file holds only the latest event.
- **Progress report accepted** iff it was written at or after the turn start (no turn start known → accepted).
- **Step** = the accepted report's `--step`, else none. Progress comes only from agent reports; the transcript is read for its activity time only, never for its content.
- **ETA remaining** = eta − (now − reported at), signed; negative is overdue.
- **Fraction** (bar), in permille: with an accepted ETA and a turn start, elapsed / (elapsed + max(0, remaining)); else step done / total; else none.
- Signals exist only for Working sessions.

### Progress file contract (plugin → app, extension)

`~/Library/Application Support/app.aydy/progress/<session-id>.json`, written atomically (temp file in the same dir, then `mv`):

```json
{ "etaSeconds": 360, "step": { "done": 4, "total": 7 } }
```

`step` is omitted without `--step`. The file's mtime is the report time. Readers skip a file whose stem is not a valid session id (`[A-Za-z0-9_-]{1,128}`), whose JSON lacks a non-negative integer `etaSeconds`, and drop a `step` unless `1 ≤ total` and `0 ≤ done ≤ total`. The session id comes from `$CLAUDE_CODE_SESSION_ID`, which Claude Code sets in every Bash tool call (subagents inherit the parent's id, so their reports land on the parent session, which is what the owner sees).

### Rust

- `session/signals.rs` (new, pure): `STALE_AFTER`, `Step { done, total }`, `ProgressReport { reported_at: Instant, eta: Duration, step: Option<Step> }`, `Eta { remaining_ms: i64, reported_ago: Duration }`, `WorkSignals { stale, quiet_for: Option<Duration>, turn_elapsed: Option<Duration>, step: Option<Step>, eta: Option<Eta>, fraction_permille: Option<u16> }`, `SignalInputs`, `derive(inputs, now)`.
- `Session` gains `transcript_written_at: Option<Instant>`, `progress: Option<ProgressReport>`, setters, and `work_signals(now) -> Option<WorkSignals>` (Some only while Working). `SessionTiming::turn_started_at()` is exposed.
- `discovery/transcript.rs`: `last_activity(transcript, session_id) -> Option<SystemTime>` (uncached stats, run every scan, because the stamp cache only watches the main file). `TranscriptFacts` and the status windows are unchanged.
- `discovery/reconciler.rs`: `Scan.activity`; `enrichment(scan, now_wall)` fills `SessionEnrichment.transcript: Option<TranscriptActivity { written_ago: Duration }>`; the registry converts the age to an `Instant` (as `apply_status` does) so the registry stays on the injected monotonic clock.
- `discovery/status_files.rs`: `DataDir::progress_dir`, `ProgressRecord`, `read_progress`. `discovery/file_feed.rs` reads progress files every pass (0.5 s), forwards only changed mtimes for live sessions, and `apply_changes` applies them after status records via `SessionRegistry::apply_progress(record, now_ms) -> bool`. A report for a session the registry does not know yet (the 25 s scan has not created it) is not applied; `apply_changes` returns those session ids and the feed forgets their forwarded mtime (`FileFeed::forget_progress`), so the next pass forwards the report again until the session exists.
- `SessionSnapshot.signals: Option<WorkSignals>`; `SessionPayload.signals: Option<SignalsPayload>` (camelCase: `stale`, `quietMs`, `turnElapsedMs`, `step {done,total}`, `eta {remainingMs, reportedAgoMs}`, `fractionPermille`, nullable where optional).
- `tray/icon.rs`: `IconState::Stale` between `SomethingNew` and `Working`, image `icons/tray/stale.png` (D1), count = stale sessions.

### Popover (Vue)

- `src/types/WorkSignals.ts` mirrors `SignalsPayload`; `Session.signals: WorkSignals | null`.
- `StatusDot` takes `stale`; stale draws a hollow ring in `status-working` (the dot still owns the colour).
- `SessionProgressLine` (new molecule) renders the second line of a Working row with signals: stale → **Quiet Nm** (`neutral-6`) then the progress text; otherwise a 48px × 2px bar (`neutral-3` track, `neutral-6` fill, width from `fractionPermille`, absent with no fraction) then the progress text, `text-0 neutral-5`, indented `pl-5` under the title. Progress text: `[d/t]` and `[~X left | X past ETA]` joined by `·`, or `X elapsed` with no report, via `formatDuration`.
- `SessionRowDetail` adds, for Working rows with signals: `Quiet X · nothing written to the transcript` when stale, `This turn X`, then `ETA reported X ago` / `No ETA reported`, before the two existing durations.
- menubar-ux.md: the one-line-row rule gains the Working-row exception; the icon table gains Stale; ui-conventions.md is unchanged except a note that the bar's width is the one inline style.

### VS Code extension

- `workSignal.ts` (new, pure): the shared rule, `parseProgress`, `sessionSignal`, `mostUrgent` (stale with the longest quiet; else shortest ETA remaining, overdue first; else longest elapsed), `titleSignal` → `{ stale, text }`. Title durations are whole minutes (`<1m`, `6m`, `1h 10m`) so the workspace file is rewritten at most about once a minute; overdue reads `2m over`.
- `sessionState.ts`: `SessionStatus.transcriptPath`; `WindowGlance.signal`; `tabTitle` renders `◌ <text>` / `◐ <text>` when the window has a signal and nothing unread. Turn start in the extension follows the shared rule via the pure `nextTurnStart(previous, status)` in `workSignal.ts`; `windowTracker.ts` keeps the per-session value between passes.
- `sessionStore.ts`: `readProgress`, `transcriptActivity` (stats only: the transcript's and its subagents' `.jsonl` mtimes; no transcript content is read), progress files pruned with the others. `windowTracker.ts` computes the window's signal each pass and watches the progress dir.
- Window registry files gain `workspace: string | null` (realpath of the open `.code-workspace`, read tolerantly so older files parse as `null`). `autoReopen.workspaceOpenElsewhere` finds another live window whose workspace is this folder's sibling or legacy workspace path; `extension.ts` then shows an information message instead of reopening, both on startup and from **Show Session Count in Tab Title**.

### Plugin

Repository root `.claude-plugin/marketplace.json` (marketplace `aydy`, plugin source `./plugin`); `plugin/.claude-plugin/plugin.json` (plugin `aydy`); `plugin/skills/progress/SKILL.md` (skill `progress`, invoked by the model); `plugin/skills/progress/scripts/aydy-progress` (POSIX sh, no dependencies; when `$HOME/Library/Application Support/app.aydy` does not exist — no AYDY installed, or Linux — it creates nothing and exits 0 with a short message); `plugin/README.md` (install with `/plugin marketplace add ericvera/aydy` and `/plugin install aydy@aydy`, usage, the mise line). AYDY does not write anything into `~/.claude`.

### Removed

Nothing is removed. The four-state tray table becomes five; `tabTitle`'s glance gains a field.

## Hard-to-undo

- New on-disk contract `app.aydy/progress/<session-id>.json` (format above), written by the plugin in every repository that installs it and read by the app and the extension. Changing it later needs both readers to accept the old shape until installed plugins update.
- Public plugin surface once pushed: marketplace `aydy`, plugin `aydy`, skill `aydy:progress`, command `aydy-progress --eta/--step`. Renaming any of them breaks existing installs and the mise-config line the README tells other repositories to paste.
- Data writes to the owner's files: the extension rewrites the `.code-workspace` title (existing mechanism) more often — at most about once a minute per window while a session in it is working.
- No schema, migration, money, hook or `~/.claude/settings.json` change; no repo-wide commands.
- Nothing is installed into the owner's own VS Code profile, `~/.claude` or real `app.aydy` by any task. The extension substitute checks (03_03, 03_04) run the packaged extension only in an isolated VS Code instance (`--user-data-dir` and `--extensions-dir` in a scratch directory, `HOME` pointed at a scratch home, scratch folders only), deleted afterwards.

## Task index

| Task file                                    | What it does                                                             | Files                                                                                                                                                                                                                      |
| -------------------------------------------- | ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `01_01_session_work_signals.md`              | Pure signal derivation and the per-session state it reads                | `src-tauri/src/session/signals.rs`, `src-tauri/src/session/session.rs`, `src-tauri/src/session/timing.rs`, `src-tauri/src/session/mod.rs`                                                                                  |
| `01_02_transcript_activity.md`               | Transcript + subagent activity time                                      | `src-tauri/src/discovery/transcript.rs`                                                                                                                                                                                    |
| `01_03_registry_signals_from_scan.md`        | Scan carries activity into the registry; snapshot carries signals        | `src-tauri/src/session/event.rs`, `src-tauri/src/session/registry.rs`, `src-tauri/src/discovery/reconciler.rs`                                                                                                             |
| `01_04_progress_files_in_feed.md`            | Read progress files every feed pass and apply them                       | `src-tauri/src/discovery/status_files.rs`, `src-tauri/src/discovery/file_feed.rs`, `src-tauri/src/session/registry.rs`                                                                                                     |
| `01_05_payload_and_stale_tray_icon.md`       | Signals on the wire; Stale tray state and D1 icon                        | `src-tauri/src/ipc/snapshot_channel.rs`, `src-tauri/src/tray/icon.rs`, `src-tauri/icons/tray/stale.png`, `docs/design/menubar-ux.md`                                                                                       |
| `02_01_popover_progress_line.md`             | Working-row progress line and hollow stale dot                           | `src/types/WorkSignals.ts`, `src/types/Session.ts`, `src/components/atoms/StatusDot.vue`, `src/components/molecules/SessionProgressLine.vue`, `src/components/molecules/SessionRow.vue`, `docs/design/ui-conventions.md`   |
| `02_02_hover_detail_signals.md`              | Hover detail gains stale, turn time and ETA report age                   | `src/components/molecules/SessionRowDetail.vue`, `src/components/molecules/SessionRow.vue`, `docs/design/menubar-ux.md`                                                                                                    |
| `03_01_plugin_progress_skill.md`             | Marketplace, plugin, `aydy:progress` skill and `aydy-progress` command   | `.claude-plugin/marketplace.json`, `plugin/.claude-plugin/plugin.json`, `plugin/skills/progress/SKILL.md`, `plugin/skills/progress/scripts/aydy-progress`, `plugin/README.md`, test `extension/src/progressScript.test.ts` |
| `03_02_extension_signal_derivation.md`       | Extension mirror of the shared rule, plus the store reads behind it      | `extension/src/workSignal.ts`, `extension/src/sessionState.ts`, `extension/src/sessionStore.ts`, `extension/src/aydyPaths.ts`, `extension/src/extension.ts`                                                                |
| `03_03_extension_title_signals.md`           | Window tracker computes the window's signal and writes it into the title | `extension/src/windowTracker.ts`, `extension/src/extension.ts`, `extension/README.md`                                                                                                                                      |
| `03_04_folder_window_workspace_elsewhere.md` | Folder window says its workspace is open elsewhere instead of reopening  | `extension/src/windowRegistry.ts`, `extension/src/sessionState.ts`, `extension/src/autoReopen.ts`, `extension/src/extension.ts`                                                                                            |
