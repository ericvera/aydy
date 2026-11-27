# 01_02 Extension: tab shows working while subagents run; informational notifications ignored

## Goal

In the AYDY VS Code extension (`extension/`), a session whose last hook payload is a `Stop` with a running `subagent` in `background_tasks` counts as working. Its window title shows `◐` and it is not unread. Informational notifications (`notification_type` `agent_completed` or `auth_success`) are neither unread nor active. The bundled hook script stops writing informational notifications, so the previous payload stays authoritative. This matches the Rust app's rules from task `01_01`.

## Files to modify/create

- `extension/src/sessionState.ts`
- `extension/src/autoReopen.ts`
- `extension/hook/aydy-hook.sh`

Tests are updated in `extension/src/sessionState.test.ts`, `extension/src/autoReopen.test.ts` and `extension/src/hookScript.test.ts`.

## Background

- Claude Code hooks run `extension/hook/aydy-hook.sh`, installed by `installHookScript` (`extension/src/hookInstall.ts:49-61`, which rewrites the installed copy whenever its content differs). The script writes the raw payload to `~/Library/Application Support/app.aydy/status/<session_id>.json`, keeping only the last one (`aydy-hook.sh:12`). It extracts fields with the `field` sed helper (`:7`).
- The extension reads these files (`extension/src/sessionStore.ts:60-82`) through `parseStatus` (`extension/src/sessionState.ts:31-59`). Today that function keeps only `session_id`, `hook_event_name` and `cwd`.
- `isWorking` (`sessionState.ts:121-122`) is `event === 'UserPromptSubmit'` only. A `Stop` sent while a subagent still runs therefore reads as idle or unread, and `tabTitle` (`:137-145`) shows `○` or `●`. In a focused window, `windowTracker.ts:206-217` auto-marks it seen, which leaves `○`.
- `isUnread` (`:61-66`) treats every `Stop` / `Notification` newer than the seen time as unread.
- `autoReopen.ts:20-21` `isActive(event)` decides whether reopening a window would interrupt a session. `countActive` (`:24-32`) maps statuses to events, and `extension.ts:264` calls `countActive`.
- Live payload example for the bug: `{"hook_event_name":"Stop", …, "background_tasks":[{"id":"a4cf…","type":"subagent","status":"running","agent_type":"mise:runner"}]}`. Finished turns carry `"background_tasks": []`.

Design decisions (from the spec):

- Only `background_tasks` entries with `type === 'subagent'` and `status === 'running'` count. Missing, non-array or malformed values count as none.
- Informational types are exactly `agent_completed` and `auth_success`. Any other type, or no type, is a real notification.
- The hook script discards informational notifications. The readers still handle them, because older scripts or old files can hold them.
- No hook registrations change. `hookSettings.ts` is not touched and `~/.claude/settings.json` is not rewritten.
- Task `01_01` already updated `docs/design/menubar-ux.md` with these rules.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `sessionState.ts`:
   - Extend `SessionStatus` with `subagentsRunning: boolean` and `informational: boolean`.
   - Add `const INFORMATIONAL_NOTIFICATION_TYPES: ReadonlySet<string> = new Set(['agent_completed', 'auth_success'])`.
   - Add `const subagentsRunning = (value: unknown): boolean`. It is true when `Array.isArray(value)` and some element is a non-null object with `type === 'subagent'` and `status === 'running'`.
   - In `parseStatus`, set `subagentsRunning: event === 'Stop' && subagentsRunning(record['background_tasks'])` and `informational: event === 'Notification' && INFORMATIONAL_NOTIFICATION_TYPES.has(String(record['notification_type']))`. Guard the type check so a non-string does not match.
   - `isWorking = status.event === 'UserPromptSubmit' || (status.event === 'Stop' && status.subagentsRunning)`.
   - `isUnread` requires `(event === 'Stop' && !subagentsRunning) || (event === 'Notification' && !informational)`, and the event must still be newer than the seen time.
2. `autoReopen.ts`:
   - Change `isActive` to take `status: SessionStatus | undefined`. It returns `status !== undefined && (isWorking(status) || (status.event === 'Notification' && !status.informational))`.
   - Change `countActive` to map session id to status.
   - Update the doc comments. Import `isWorking` from `./sessionState`. Drop the `SessionEvent` import if it becomes unused.
3. `aydy-hook.sh`:
   - Change the header comment to `# AYDY hook script v2`.
   - After the `SessionEnd` branch and before `mv`, add:
     ```sh
     if [ "$event" = Notification ]; then
       case "$(field notification_type)" in
         agent_completed|auth_success) rm -f "$tmp" 2>/dev/null; exit 0 ;;
       esac
     fi
     ```
   - Keep it POSIX `/bin/sh` and silent: no stdout or stderr, always exit 0.

## Gotchas

- `parseStatus` tests use `toEqual` on the whole object. Update the existing expectations to include `subagentsRunning: false, informational: false`. The local `status(...)` test helper in `sessionState.test.ts` and any `SessionStatus` literals in other tests (`autoReopen.test.ts`, `windowTracker.test.ts`, `sessionStore.test.ts`) must gain the two fields or `yarn build-tsc` fails. Grep for `eventAt:` across `extension/src/*.test.ts`.
- `hookScript.test.ts`'s `runHook` asserts empty stdout and stderr and exit 0. The new branch must not print anything.
- The script's `field` helper only matches `[A-Za-z0-9_-]` values, which covers these tokens.
- `yarn lint` (ESLint and Prettier) must pass. Follow the existing formatting: no semicolons, single quotes, trailing commas.

## Verification

Run from the repo root: `yarn build-tsc`, `yarn lint` and `yarn test`. Also run `cargo test` and `cargo clippy` from `src-tauri/` to confirm the suite stays green.

Tests to write:

- `extension/src/sessionState.test.ts` (regression):
  - `parseStatus` on a `Stop` with `background_tasks: [{type:'subagent',status:'running'}]` gives `subagentsRunning: true`. That status is working: `windowGlance` gives `{ unread: 0, working: true }` and `tabTitle` gives `◐ aydy`.
  - A `Stop` with only a running `shell`, with `[]`, with a malformed `background_tasks`, or with none is not working and is unread.
  - A `Notification` with `notification_type: 'agent_completed'` or `'auth_success'` gives `informational: true` and is neither unread nor working. A `permission_prompt` notification is still unread.
- `extension/src/autoReopen.test.ts`:
  - `isActive` is true for `UserPromptSubmit`, a needs-input `Notification`, and a `Stop` with subagents running.
  - `isActive` is false for a plain `Stop`, an informational `Notification`, `SessionStart` and `undefined`.
  - Update `countActive` accordingly.
- `extension/src/hookScript.test.ts`:
  - After writing a `Stop` payload, running the script with a `Notification` whose `notification_type` is `agent_completed` (and again for `auth_success`) leaves the status file byte-identical to the `Stop` payload.
  - A `permission_prompt` notification overwrites it.
