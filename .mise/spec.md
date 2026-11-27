# Spec: subagent working state and done sound

## What

- A Claude Code `Stop` sent while any `background_tasks` entry has `type: "subagent"` and `status: "running"` means the session is still working. The VS Code tab shows `◐`, the AYDY app shows Working, and no Finished sound plays.
- A `Stop` with no running subagent is Done, as today. The Finished sound then follows the existing unread and cooldown rules.
- Running background tasks of any other type (for example `shell`) do not keep a session working.
- Informational notifications (`notification_type` of `agent_completed` or `auth_success`) change no status, raise no unread, and play no sound. Every other notification, including permission prompts, idle / input-needed prompts and notifications with an unknown or missing type, behaves as today: NeedsInput, unread, sound.
- A missing or malformed `background_tasks` counts as no running subagents.
- No new hooks are registered. `~/.claude/settings.json` is not rewritten.

## Design

One raw payload per session is the shared input. The hook script keeps only the last payload in `~/Library/Application Support/app.aydy/status/<id>.json`. Both readers derive state from it: Rust through `HookPayload`, the extension through `parseStatus`.

### Rust app (`src-tauri/`)

- `session/hook_payload.rs`: `HookPayload` gains `background_tasks: Option<serde_json::Value>`. It is typed as `Value` so a malformed field cannot fail the whole payload. A helper `subagents_running(&Value) -> bool` is true when the value is an array holding an object with `"type": "subagent"` and `"status": "running"`. In `into_event`:
  - `Stop` with running subagents maps to `HookEventKind::SubagentsRunning`. Any other `Stop` maps to `HookEventKind::Stop`.
  - `Notification` whose `notification_type` is in `INFORMATIONAL_NOTIFICATION_TYPES = ["agent_completed", "auth_success"]` maps to `HookEventKind::Informational`. Any other notification maps to `Notification { message }` as today.
- `session/event.rs`: `HookEventKind` gains `SubagentsRunning` (the main turn stopped while subagents still run, so the session is still working) and `Informational` (a notification that asks nothing of the user).
- `session/session.rs` `apply_hook`:
  - `SubagentsRunning` sets `waiting = false` and `turn_open = true`, and calls `timing.start_turn(now)`. That call is idempotent, so a turn already open keeps its start.
  - `Informational` records no fact beyond the common hook bookkeeping (`hook_tracked`, `presence_confirmed`, `reported_gone = false`, `last_event_at`).
- `session/registry.rs` `apply_status`: `Stop` and `Notification` set `attention_at_ms` as today. `Informational` leaves `attention_at_ms` untouched, so an earlier unread is neither raised nor cleared. Every other event, `SubagentsRunning` included, clears it.
- `notify/` is unchanged. Sounds come from status transitions (`transition_alert`), so Working→Working plays nothing and Working→Done on the final `Stop` plays Finished. Tests are added there.
- `discovery/file_feed.rs` is unchanged. Its one-pass hold applies only to `Stop | Notification { .. }`, and the new variants need no hold because they raise no attention.
- Status is still derived from facts and never stored (`session/status.rs`).

### VS Code extension (`extension/`)

- `src/sessionState.ts`: `SessionStatus` gains `subagentsRunning: boolean` and `informational: boolean`, both parsed by `parseStatus` with the same rules and type list as Rust.
  - `isWorking` is `UserPromptSubmit`, or `Stop && subagentsRunning`.
  - `isUnread` requires `(Stop && !subagentsRunning) || (Notification && !informational)`, and the event must still be newer than the seen time.
- `src/autoReopen.ts`: `isActive` takes a `SessionStatus | undefined` instead of an event. It is `isWorking(status) || (Notification && !informational)`. `countActive` passes the status.
- `hook/aydy-hook.sh` (header bumped to `v2`): a `Notification` whose `notification_type` is `agent_completed` or `auth_success` is discarded without touching the status file. The previous payload therefore stays authoritative, for example a `Stop` with a running subagent. Without this, the extension (which keeps no history between reads) would flip `◐` to `○` when a subagent completes. The readers still handle informational payloads defensively, because an older installed script can still write them.
- The existing `installHookScript` (`src/hookInstall.ts:49-61`) rewrites the installed script when its content differs. No install code changes.

### Docs

- `docs/design/menubar-ux.md` "What clears an unread" gains the two exceptions: a `Stop` with running subagents is Working and not unread, and informational notifications do not count. The tab-title sentence notes that `◐` covers a session whose subagents are still running.

### Removed

Nothing is removed.

## Hard-to-undo

- On activation, the extension's existing `installHookScript` overwrites the installed hook script at `~/Library/Application Support/app.aydy/bin/aydy-hook.sh` on users' machines with the v2 script. Reverting means shipping the old script again. `~/.claude/settings.json` is not touched.

## Task index

| Task file                              | What it does                                                                                                                                                                                   | Files                                                                                                                                                                                                                           |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `01_01_rust_subagents_working.md`      | Rust: parse `background_tasks` and informational notification types, add `SubagentsRunning` / `Informational` event kinds, keep sessions Working and silent while subagents run; update UX doc | `src-tauri/src/session/hook_payload.rs`, `src-tauri/src/session/event.rs`, `src-tauri/src/session/session.rs`, `src-tauri/src/session/registry.rs`, `docs/design/menubar-ux.md` (tests also in `src-tauri/src/notify/sound.rs`) |
| `01_02_extension_subagents_working.md` | Extension: parse the same facts, show `◐` while subagents run, ignore informational notifications, and have the hook script drop informational notifications                                   | `extension/src/sessionState.ts`, `extension/src/autoReopen.ts`, `extension/hook/aydy-hook.sh`                                                                                                                                   |
