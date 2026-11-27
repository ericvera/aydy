# 01_01 Rust: sessions with running subagents stay Working; informational notifications are silent

## Goal

In the AYDY Tauri app (`src-tauri/`), a Claude Code `Stop` hook payload whose `background_tasks` contains a running `subagent` keeps the session Working. Such a session is not unread and plays no Finished sound. A `Notification` whose `notification_type` is informational (`agent_completed`, `auth_success`) changes no status, raises no unread and plays no sound. Every other behavior stays the same.

## Files to modify/create

- `src-tauri/src/session/hook_payload.rs`
- `src-tauri/src/session/event.rs`
- `src-tauri/src/session/session.rs`
- `src-tauri/src/session/registry.rs`
- `docs/design/menubar-ux.md`

Test code is also added to `src-tauri/src/notify/sound.rs` (its `#[cfg(test)]` module only).

## Background

- Claude Code hooks write one JSON payload per session into `~/Library/Application Support/app.aydy/status/<session_id>.json`, and only the last payload is kept. The app reads these files every 500 ms (`src-tauri/src/discovery/file_feed.rs`). It parses each with `HookPayload` (`src-tauri/src/discovery/status_files.rs:69-88`) and applies the result through `SessionRegistry::apply_status` (`src-tauri/src/session/registry.rs:168-183`).
- Live data shows that the main session's `Stop` arrives while a subagent still runs. Example:
  `{"hook_event_name":"Stop", ..., "background_tasks":[{"id":"a4cf…","type":"subagent","status":"running","description":"…","agent_type":"mise:runner"}]}`. Finished turns carry `"background_tasks": []`.
- Today `HookPayload` (`hook_payload.rs:14-30`) ignores `background_tasks`, and `"Stop"` maps to `HookEventKind::Stop` (`:48`). `Session::apply_hook` (`session.rs:128-132`) then closes the turn, which makes the session Done. `notify::transition_alert` (`notify/mod.rs:31-40`) turns Working→Done into a Finished alert, and `SoundNotifier::observe` (`notify/sound.rs:262-320`) plays it for unread sessions.
- Any `Notification` sets `waiting = true` (`session.rs:124-127`), which means NeedsInput and the needs-input sound. The known `notification_type` tokens are listed in the comment at `hook_payload.rs:41-44`.
- Status is derived from `StatusFacts` and never stored (`session/status.rs:44-80`). Keep it that way: record facts, never a status.
- `HookPayload` lets every field be optional so an odd payload still parses (doc comment `hook_payload.rs:9-13`). A typed `Vec<Struct>` for `background_tasks` would fail the whole payload when the field is malformed, so use `serde_json::Value`.

Design decisions (from the spec):

- Add `HookEventKind::SubagentsRunning` and `HookEventKind::Informational` to `session/event.rs`. Leave `Stop` and `Notification { message }` as they are, because many tests construct them.
- `INFORMATIONAL_NOTIFICATION_TYPES = ["agent_completed", "auth_success"]`. Every other type, or no type, stays a needs-input `Notification`.
- `Informational` leaves `attention_at_ms` untouched, neither setting nor clearing it. `SubagentsRunning` clears it, like any non-attention event.
- Do not change `notify/` production code or `discovery/file_feed.rs`. The file-feed hold (`file_feed.rs:97-105`) matches `Stop | Notification { .. }` only, which is correct for the new kinds.
- The VS Code extension applies the same rules in task `01_02`. The two must agree.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `event.rs`: add the two variants to `HookEventKind` with doc comments in the existing style:
   - `SubagentsRunning`: "A turn's `Stop` arrived while subagents it started are still running; the session is still working."
   - `Informational`: "A notification that tells the user something but asks nothing of them, such as a background agent completing."
     Update the enum's doc comment ("The five Claude Code hook events…") so it says these are the kinds the five registered hooks produce.
2. `hook_payload.rs`:
   - Add `pub background_tasks: Option<serde_json::Value>` with a doc comment. It is the `Stop` payload's background work, kept as raw JSON so a malformed value counts as none instead of failing the payload.
   - Add `const INFORMATIONAL_NOTIFICATION_TYPES: [&str; 2] = ["agent_completed", "auth_success"];` and `fn subagents_running(tasks: Option<&serde_json::Value>) -> bool`. It is true iff the value is an array with an element that is an object where `type == "subagent"` and `status == "running"`, both compared as strings.
   - In `into_event`:
     - `Some("Stop")` maps to `SubagentsRunning` when `subagents_running(self.background_tasks.as_ref())`, otherwise to `Stop`.
     - `Some("Notification")` maps to `Informational` when `notification_type` (trimmed) is in the list, otherwise to today's `Notification { message }`.
   - Watch the move semantics: `self.notification_type` is consumed by the fallback, so check it by reference first.
3. `session.rs` `apply_hook` match:
   - `HookEventKind::SubagentsRunning`: `self.facts.waiting = false; self.facts.turn_open = true; self.timing.start_turn(now);`.
   - `HookEventKind::Informational`: no fact changes. It can join the `SessionStart | SessionEnd` arm.
   - `notification_message` is not touched by either.
4. `registry.rs` `apply_status`: replace the bool `attention` computation with three cases:
   - `Stop | Notification { .. }` sets `Some(written_at_ms)`.
   - `Informational` keeps the current value.
   - Anything else sets `None`.
     Only assign `session.attention_at_ms` when it is not the keep case. Update the doc comment (`:165-167`) to mention the informational exception.
5. `docs/design/menubar-ux.md`, section "What clears an unread" (around line 221):
   - After the first sentence, add that a `Stop` sent while subagents are still running is not a finished turn: the session stays Working and is not unread until a `Stop` with no running subagents arrives.
   - Add that informational notifications (a background agent completing, a sign-in succeeding) neither raise nor clear an unread and play no sound.
   - In the final paragraph, note that `◐` also shows while a session's subagents are still running.
     Keep the doc's tone and line wrapping (80 columns).

## Gotchas

- Do not use `#[serde(deny_unknown_fields)]` or a typed struct for `background_tasks`, because a malformed value must not drop the payload. Test `"background_tasks": "oops"` and `"background_tasks": [1, null, {"type":"subagent"}]` (no status), and both must give `Stop`.
- A running `shell` (any non-`subagent` type) must give `Stop`.
- `start_turn` is already idempotent (`session/timing.rs:40-44`). Do not call `end_turn` for `SubagentsRunning`.
- Every exhaustive `match` on `HookEventKind` must compile. Today that is `session.rs:117` only, but run `cargo clippy` to be sure.
- `cargo clippy` runs as part of Check and must stay clean (for example, no needless borrows in the helper).

## Verification

Run from `src-tauri/`: `cargo test` and `cargo clippy`. Also run `yarn build-tsc`, `yarn lint` and `yarn test` from the repo root to confirm nothing else broke.

Tests to write:

- `src-tauri/src/session/hook_payload.rs` tests:
  - A `Stop` with `background_tasks: [{type:"subagent",status:"running"}]` gives `HookEventKind::SubagentsRunning`.
  - A `Stop` with `[]`, with a running `shell` only, with completed subagents only, or with no field gives `Stop`.
  - A malformed `background_tasks` gives `Stop`.
  - A `Notification` with `notification_type` `agent_completed` gives `Informational`, and so does one with `auth_success` (even with a message).
  - `permission_prompt` and `idle_prompt` still give `Notification { .. }`.
- `src-tauri/src/session/session.rs` tests:
  - `UserPromptSubmit` then `SubagentsRunning` is Working, and the turn timer keeps running.
  - `SubagentsRunning` then `Stop` is Done.
  - `Informational` after `UserPromptSubmit` stays Working, and after `Stop` stays Done.
- `src-tauri/src/session/registry.rs` tests (use the existing `dated_registry`, `status(..)` and `is_unread` helpers near `:1010`):
  - A `SubagentsRunning` record is not unread.
  - `Stop` (unread) followed by an `Informational` record stays unread.
  - A seen `Stop` followed by an `Informational` record stays read.
- `src-tauri/src/notify/sound.rs` tests (use `Harness`, `working`, `hook` and `observe` at `:430-517`):
  - Regression: working, then `SubagentsRunning`, then observe plays nothing. A following `Stop` and observe plays exactly `[Sound::Finished]`.
  - Working, then `Informational`, then observe plays nothing.
  - Done, then `Informational`, then observe plays nothing.
  - A needs-input `Notification` still plays `NeedsInput` (already covered, so keep it passing).
