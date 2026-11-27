I am seeing that sometimes, even though agents are working, the window tab name shows the open circle icon. Could that be related to agents? Is it possible to also not make sound for intermediary messages and only do it when the main session is "done" and there is no sub-agent still working?

## Investigation

Reproduced: yes, from live data (read-only). A session status file under `~/Library/Application Support/app.aydy/status/` held `hook_event_name: "Stop"` with `background_tasks: [{type:"subagent", status:"running", agent_type:"mise:investigator"}]` — the main turn's Stop is recorded while a subagent is still running. Other Stop files have `background_tasks: []`.

- Root cause, tab: `extension/src/sessionState.ts:121` `isWorking` is `event === 'UserPromptSubmit'` only. A Stop with running background_tasks reads as not working, and `tabTitle` (:137-145) shows `○` (or `●` when unread). `parseStatus` (:31-59) drops every field except session_id/event/cwd. In a focused window, `windowTracker.ts:206-217` auto-marks the Stop seen, so the user gets `○`.
- Root cause, app/sound: `src-tauri/src/session/hook_payload.rs:16-30` does not deserialize `background_tasks`; `:48` maps Stop to `HookEventKind::Stop`. `session.rs:128-132` closes the turn → Done. `notify/mod.rs:31-40` `transition_alert` and `notify/sound.rs:161`/`:262-320` play Finished on Working→Done for an unread session.
- Intermediary sounds: any `Notification` sets `waiting=true` (`session.rs:124-127`) → NeedsInput → sound. `hook_payload.rs:41-44` lists `agent_completed` among notification types; inferred (not observed) that a background agent finishing fires one.
- Not verified: whether the main session's re-entry after a task-notification fires `UserPromptSubmit`. If not, the session stays Done/○ while it processes subagent results.
- Reconciliation can't correct it: transcript status is adopted only for non-hook-tracked or Unknown sessions (`registry.rs:243-246`).
- Hooks registered: `extension/src/hookSettings.ts:3-9` (SessionStart, UserPromptSubmit, Notification, Stop, SessionEnd), matching `session/event.rs:10-24`. SubagentStart/SubagentStop not registered. `extension/hook/aydy-hook.sh:12` keeps only the last payload per session.
- Both readers parse the same raw payload: Rust `discovery/status_files.rs:69-88` via `HookPayload`, extension `sessionStore.ts:71` via `parseStatus`. A `background_tasks` running-count is available to both without changing the hook script.
- Patterns: status derived from facts, never stored (`session/status.rs:44-80`). Unread rule is "Stop/Notification newer than seen" (`registry.rs:168-183`, `sessionState.ts:61-66`, `docs/design/menubar-ux.md:223`).
- Regression tests: `extension/src/sessionState.test.ts`, `hook_payload.rs` tests (:66+), `registry.rs` tests (:498+), `notify/sound.rs` / `notify/mod.rs` tests.
- Hard to undo: adding hook events rewrites `~/.claude/settings.json` (`hookSettings.ts`, `hookInstall.ts:53-59`); legacy migration in `src-tauri/src/hooks/merge.rs`/`legacy.rs`.
- Already decided by code: sounds play only for unread sessions, with per-kind cooldown (`sound.rs:188-200`, `:296`). NeedsInput is a separate sound and toggle from Finished (`sound.rs:52-77`).

## Answers

- Sounds while subagents run: permission / input-needed prompts still play a sound; `agent_completed` and other informational notifications do not.
- Only running background tasks of type `subagent` keep a session working; background shells do not.
- The AYDY menu bar app also shows the session as Working while subagents run, so the tab and the app always agree.

## Repro

1. In a Claude Code session in VS Code, start a background subagent and let the main turn end while the subagent is still running.
2. Observe: the status file holds `Stop` with a running `subagent` in `background_tasks`; the tab shows `○` (or `●`), the app shows Done, and the Finished sound plays.

## Expected

- While any `subagent` background task is running, the session is Working: tab shows `◐`, the app shows Working, and no Finished sound plays.
- A `Stop` with no running subagents is Done, and only then is the Finished sound played (existing unread/cooldown rules unchanged).
- Informational notifications (e.g. `agent_completed`) play no sound; permission / input-needed notifications still do.

## Regression tests

- `extension/src/sessionState.test.ts`: a Stop with a running subagent is working (`◐`); a Stop with only a running shell, or none, is not.
- `src-tauri/src/session/hook_payload.rs` tests: parses `background_tasks`.
- `src-tauri/src/session/registry.rs` / `src-tauri/src/notify/` tests: no Finished alert while subagents run; one when the final Stop has none; no sound for informational notifications.

## Assumptions

- No new hooks are registered (no SubagentStart/SubagentStop); the fix reads `background_tasks` from the existing payload, so `~/.claude/settings.json` is not rewritten.
- A missing or malformed `background_tasks` field counts as no running subagents.
- No mock: the change reuses existing states and icons (`◐`, Working) and adds no new UI.
- Whether a new `Stop` without subagents follows the re-entry after a task notification is not verified; the fix relies on the last payload alone, so a session left on a Stop-with-subagents payload stays Working until the next payload arrives.

## Proposal

Issue: a `Stop` sent while subagents are still running is treated as done, so the tab shows `○`, the app shows Done, and the Finished sound plays early; informational notifications also play the needs-input sound.
Fix: in both the extension and the app, parse `background_tasks`, treat Stop with a running `subagent` as Working, sound Finished only on a Stop with none, and silence informational notifications.
Skips: nothing; spec and critic run because the fix spans the extension and the Rust app.
