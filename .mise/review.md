# Review

## Tasks

- 01_01 (5359aa5) Rust app: `Stop` with a running `subagent` in `background_tasks` → new `SubagentsRunning` event keeps the session Working; `agent_completed`/`auth_success` → `Informational`, which changes no facts or attention. Verify: `cargo test` (hook_payload, session, registry, notify/sound regression tests); live: start a background subagent, let the main turn end — app shows Working, no Finished sound until the final Stop. Doc: `docs/design/menubar-ux.md` "What clears an unread".
- 01_02 (281c06a) Extension + hook script v2: `SessionStatus` gains `subagentsRunning`/`informational`; `isWorking` covers Stop-with-subagents (tab `◐`); `isUnread`/`autoReopen.isActive` ignore informational; `aydy-hook.sh` v2 drops `agent_completed`/`auth_success` so the prior payload stays. Verify: `yarn test` (sessionState, autoReopen, hookScript tests); live: tab stays `◐` while subagents run, then `○`/`●` after the final Stop.

## Reviewer minors (not fixed)

- `extension/README.md:87-88` still says `○` whenever no prompt is running; should mention subagents still running.
- `extension/README.md:43-44` says every hook event is written; v2 drops informational notifications.
- `extension/src/sessionState.ts:48` doesn't trim `notification_type`, Rust does (`hook_payload.rs:51`).

## Open assumptions

- No new hooks registered; `~/.claude/settings.json` untouched. The installed hook script is replaced with v2 on extension activation.
- Missing/malformed `background_tasks` = no running subagents. Background shells never count.
- Unverified: a fresh `Stop` follows the main session's re-entry after a task notification; if not, a session stays Working until its next payload.
- Baseline flake: `extension/src/windowTracker.test.ts:446` failed once, passed on rerun.

## Amendments

None.
