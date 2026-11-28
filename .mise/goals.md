there are 2 things I am having difficulty with. 1) the status of agents in terms of "is it stale or stuck" 2) some signal of how long until done and/or how much work is left. I am open to creative ideas for both whether it is through the current surfaces of aydy or a new one.

## Investigation

New feature work; nothing to reproduce.

1. Liveness gap: the hook registers only SessionStart, UserPromptSubmit, Notification, Stop, SessionEnd (extension/src/hookSettings.ts:3-9). No Pre/PostToolUse, so status-file mtime cannot separate busy from stuck mid-turn (status_files.rs:56,72; registry.rs:170-174).
2. Status is derived from four booleans with a deliberate "no-decay rule" (status.rs:47-49,67-79). Stale/stuck fits as a separate signal beside status, not a fifth status (would break rank order status.rs:35-41 and menubar-ux.md icon order).
3. Heartbeat candidate: the transcript is the only file growing mid-turn; read every 25 s (reconciler.rs:42,170-189) via a size/mtime-keyed cache (transcript.rs:6-8,152,204). TranscriptFacts carries only title/branch/status (transcript.rs:70-79); mtime not exposed. Facts flow via enrichment() (reconciler.rs:319-345) and SessionEnrichment (event.rs:134-146).
4. Transcript parsing is free-form JSON, total (transcript.rs:309-348); sidechain entries skipped (318-325). Only `stop_reason == tool_use` is read today (329-336); no todo/task parsing.
5. Timing: working time and longest completed turn per session in timing.rs:12-70; open turn start `turn_started_at` is private (15,38-42). No per-session turn history.
6. Subagents: background_tasks detected only on Stop (hook_payload.rs:63-64,80-88), mirrored in extension/src/sessionState.ts:45,90,147. Task counts available but not kept.
7. New field path: Session (session.rs:16-46) → SessionSnapshot (registry.rs:34-45,368-382) → SessionPayload (src-tauri/src/ipc/snapshot_channel.rs:85-110) → TS Session (src/types/Session.ts). Frontend derives nothing about state; computation belongs in Rust.
8. UI constraints (docs/design/menubar-ux.md): row = dot + title; extra detail on hover line (src/components/molecules/SessionRowDetail.vue:39-47); status colour reserved for the dot; tray icon is monochrome template (shape only, src-tauri/src/tray/icon.rs); extension window title uses ○/◐/● (extension/src/windowTracker.ts, sessionState.ts:140-147). ui-conventions.md required before Vue changes; UI change needs a mock.
9. Hard to undo: adding hook events changes HOOK_EVENTS and rewrites the user's ~/.claude/settings.json (hookSettings.ts:96-125; hookInstall.ts); every event rewrites the status file (aydy-hook.sh:17), affecting last-event ordering (session.rs:85-89) and clearing unread for non-Stop/Notification events (registry.rs:175-180); changing aydy-hook.sh needs a "v2" header bump and reinstall.
10. Time bases: transcript scan 25 s; status files polled 0.5 s (file_feed.rs:22). Informational notifications neither raise nor clear unread (registry.rs:181).

## Decisions

1. Stale/stuck heartbeat: transcript heartbeat. Track when the session's transcript last grew; a session that is Working and whose transcript has been quiet for N minutes is flagged stale. No hook or settings.json changes. Stale is a separate signal beside status, not a fifth status (no-decay rule stays).
2. Progress / time left: the owner wants a skill that AYDY provides to agents so agents can report their own ETA (and progress) somewhere AYDY reads it.
3. Surfaces: the owner wants to see mocks of the surface options before choosing — popover line under each row with progress + "quiet Nm" and a hollow/dimmed dot when stale; the same plus the VS Code window title (e.g. "◐ 4/7"); a new popover strip of working sessions' elapsed time vs expected; tray icon shape change when any session is stale — plus other creative ideas. The mock shows each option as a separate, labelled variant side by side so the owner can pick.

4. Surfaces chosen from the mock: variant A (a line under each working popover row — progress/ETA, "Quiet Nm", or elapsed — plus a hollow dot when stale) and variant B (the same signal in the VS Code window title, ◌ glyph for stale). Also shipping the tray icon stale shape D1 from the mock (the first of variant D's two shapes; see .mise/mock/). Not shipping: C (Working now strip), D2, E1–E4.
5. When one VS Code window has several working sessions, its title shows the most urgent one: stale first, then the shortest ETA.

6. The progress skill ships as a Claude Code plugin from this repo's own marketplace (like mise), installed with `/plugin install`; the skill and its `aydy-progress` command live inside the plugin. AYDY does not write the skill into ~/.claude. This replaces the earlier assumption that AYDY installs it beside the hook script.
7. The skill has no `--doing` option; it reports only `--eta` and optional `--step`.
8. Folder-window fix (in scope): when a window opened as a plain folder has its sibling `.code-workspace` already open in another VS Code window (known from the app.aydy/windows registry), AYDY tells the owner the folder is already open as a workspace in another window instead of calling openFolder (which cannot reuse this window and just focuses the other one). Found when this repo's folder window showed no title status while a second window had aydy.code-workspace open.

9. mise integration is documentation only: the plugin's README shows a Skills & guides line a repo can paste into its `.claude/mise-config.md` (e.g. `- aydy:progress (skill): at the start of each task and after each step`). No change to mise itself, now or as a planned follow-up.

10. No TodoWrite checklist fallback. Progress comes only from agent reports via `aydy:progress`; without a report, AYDY shows the current turn's elapsed time. (Critic found 0 TodoWrite or task-list calls across all transcripts on this machine.)

## Assumptions

- Agent-reported ETA is the primary "time left" signal; when an agent has not reported one, AYDY shows the current turn's elapsed time.
- An agent-reported ETA older than the turn it was reported in (a new prompt started since) is discarded.
- Stale threshold N defaults to a fixed value (e.g. 5 minutes) chosen in the spec; not user-configurable in this run.
- (mock) Stale ranks below the two unread states and above Working in the tray icon; the tray count for stale is the number of stale sessions.
- (mock) The "Working now" strip (variant C) uses the session's longest completed turn as expected time when no ETA was reported.
- (mock) In the VS Code window title (variant B), stale wins over progress and uses a new ◌ glyph; which session to show when one window has several working sessions is undecided.
- (mock) The skill drives a CLI `aydy-progress --eta/--step`; the `--doing` phrase is not drawn on any surface yet.
- (mock) Variant A breaks the one-line-row rule in menubar-ux.md for working rows.

## Proposal

Issue: AYDY can't tell a busy session from a stuck one, and gives no sense of how much work is left.
Approach: a transcript heartbeat flags Working sessions quiet ≥5m as stale (a separate signal, not a status); a skill shipped as a Claude Code plugin from this repo (with its `aydy-progress` command) lets agents report ETA/step, falling back to elapsed time; surfaces are a line under working popover rows with a hollow stale dot (A), the VS Code window title (B), and the tray icon's D1 shape when any session is stale; a folder window whose workspace is open elsewhere says so instead of trying to reopen; the plugin README documents a mise Skills & guides line.
Skips: none (spec and critic both run — new concept, Rust core + popover + extension + a new plugin).
