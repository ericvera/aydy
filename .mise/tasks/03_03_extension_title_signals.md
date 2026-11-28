# 03_03 Extension: work signal in the window title

## Goal

Each VS Code window's title shows its most urgent working session's signal — `◐ 4/7 · ~6m`, `◐ ~6m`, `◐ 3m`, or `◌ quiet 9m` — computed by the window tracker on every pass (variant B of the approved mock).

## Files to modify/create

- `extension/src/windowTracker.ts`
- `extension/src/extension.ts`
- `extension/README.md`

## Background

The AYDY Companion extension keeps each window's title through its generated `.code-workspace` file: `WindowTracker` (`extension/src/windowTracker.ts`) reads the status/seen/window files on every change and every 5 s (`TRACKER_POLL_MS`), works out which sessions this window owns (`pass`, `:203-246`), and calls `onGlance` when the glance changed; `extension.ts` (`activate`, `:492-541`; `writeTitle`, `:104-115`) turns the glance into a title with `tabTitle` and writes it with `setWorkspaceTitle` (which skips the write when the title is unchanged).

Task 03_02 produced, in `extension/src/workSignal.ts`: `nextTurnStart(previous, status)` (the turn-start state machine shared with Rust's `SessionTiming`; idempotent), `sessionSignal(activity, now)` (`SessionActivity.turnStartedAt` is the caller's running turn start), `mostUrgent(signals)`, `titleSignal(signal)`, `SessionActivity`, `ProgressReport`; in `extension/src/sessionStore.ts`: `readProgress(dirs)`, `transcriptActivity(transcriptPath, sessionId)` (newest write time in ms, or `null`; stats only); in `extension/src/sessionState.ts`: `SessionStatus.transcriptPath`, `WindowGlance.signal: TitleSignal | null`, `windowGlance(statuses, seen, signal)`, and `tabTitle` already rendering a signal; in `extension/src/aydyPaths.ts`: `progressDir()`.

Owner decisions: the title shows the most urgent working session — stale first, then the shortest ETA (`mostUrgent` encodes the full order); unread still takes the slot (`● 1 okven · main`), exactly as today. Approved titles (glyph, signal text, then the existing label): `◐ 4/7 · ~6m aydy · feat/x`, `◐ ~6m aydy · feat/x`, `◐ 3m aydy · feat/x` (elapsed, no report), `◌ quiet 9m aydy · feat/x` (stale, U+25CC), and plain `◐ aydy · feat/x` when there is nothing to say; `titleSignal` (03_02) already produces the text.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `windowTracker.ts`:
   - `TrackerDirs` gains `progress`; `defaultDirs` uses `progressDir()`.
   - `TrackerStore` gains `readProgress: () => Promise<Map<string, ProgressReport>>` and `transcriptActivity: (transcriptPath: string, sessionId: string) => Promise<number | null>` (newest write time); `defaultStore` binds them.
   - `start()` watches the progress dir along with the others (create it with `mkdir -p` like them).
   - `WindowTracker` keeps `private readonly turnStarts = new Map<string, number | null>()`. In `pass()`, after ownership, for **every** owned status (working or not) set `turnStarts.set(id, nextTurnStart(turnStarts.get(id) ?? null, status))`, so a Stop clears the value; delete entries whose session is no longer owned.
   - Then for each owned status where `isWorking(status)`, build a `SessionActivity` (with `turnStartedAt` from `turnStarts`) (`transcriptActivity` only when `transcriptPath !== null`, else nulls; progress from the map), compute `sessionSignal(activity, Date.now())`, then `titleSignal(mostUrgent(signals))` (null-safe), and pass it as the third argument to `windowGlance`.
   - The change check compares `unread`, `working`, and the signal (`stale` and `text`), so a title changes when its text changes and not otherwise.
   - Read failures for one session's transcript cost that session's activity only (catch per session), never the pass.
2. `extension.ts`: no logic change beyond what the compiler needs — `writeTitle` already uses `tabTitle(glance, …)`; confirm the initial `glance` literal carries `signal: null` (added in 03_02).
3. `extension/README.md`: in "What it does", describe the title signals (glyphs, what each text means, stale = nothing written by the session or its subagents for 5 minutes, the most-urgent rule, that progress comes from only the `aydy:progress` plugin skill in `plugin/`, else elapsed turn time; TodoWrite is not read), and add `progress/` to the list of files under `app.aydy/` it reads and prunes.

## Gotchas

- Title texts are minute-granular by design (03_02), so with a 5 s poll the workspace file is rewritten at most about once a minute per changing session; do not add a faster poll.
- Keep the pass bounded: one stat walk (transcript + subagents dir) per owned working session per pass; no transcript content is read.
- `windowTracker.test.ts` injects a fake `store` and `watchDir`; extend the fake with the two new methods and keep existing tests green.
- Do not compute signals for sessions this window does not own, or for non-working sessions (but do run `nextTurnStart` for every owned status, or a Stop would never clear the turn start).
- The turn start lives in memory, so a status that changes twice between passes (Stop then a new prompt) is seen as one open turn and keeps the old start; Rust's 0.5 s file feed has the same collapse at a finer grain. Accepted; do not add persistence.

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests in `windowTracker.test.ts` (fake store, fake clock via `vi.useFakeTimers()` / `vi.setSystemTime` as existing tests do):
  - a working owned session with an accepted progress report produces a glance whose signal text is `4/7 · ~6m`;
  - a working owned session whose transcript activity is 9 minutes old produces `{ stale: true, text: 'quiet 9m' }`;
  - two working sessions, one stale and one with an ETA, → the stale one wins; two with ETAs → the shorter;
  - a session owned by another window contributes nothing;
  - `onGlance` is not called again when nothing changed, and is called when only the signal text changed;
  - a transcript read that throws for one session leaves the other session's signal intact;
  - turn start carried across passes: pass 1 sees `UserPromptSubmit` at t0, pass 2 sees `Stop` with subagents running at t0+5m with a report written at t0+2m → the report is accepted (`4/7 · ~…`); a report written before t0 is ignored; after a pass that sees a plain `Stop`, the next prompt starts a fresh turn.
- Substitute check in an **isolated** VS Code instance. Never run `code --install-extension` without `--extensions-dir`, never open the owner's own folders, and never use the owner's real VS Code profile, `~/.claude` or `~/Library/Application Support/app.aydy`: the packaged build has the same version (0.1.0) as the owner's daily build, so installing it there would silently replace it, run in every open window, rewrite every sibling `.code-workspace` and run `installHookScript`/`ensureHooks` against the real home.
  1. Make `S`, a new directory under the session scratchpad (`mkdir -p "$S"`), and record the baseline: `code --list-extensions --show-versions > "$S/before.txt"` and `stat -f %m ~/.claude/settings.json`.
  2. Set up scratch dirs: `H="$S/home"`; `mkdir -p "$H/Library/Application Support/app.aydy" "$S/ext" "$S/data" "$S/folders/demo"`.
  3. `yarn package-extension`, then `env -u CLAUDE_CONFIG_DIR HOME="$H" code --extensions-dir "$S/ext" --install-extension extension/aydy-companion-0.1.0.vsix`.
  4. Launch: `env -u CLAUDE_CONFIG_DIR HOME="$H" code --user-data-dir "$S/data" --extensions-dir "$S/ext" --new-window "$S/folders/demo"` (a separate `--user-data-dir` starts a separate VS Code instance that inherits this environment).
  5. Guard: within a few seconds `"$H/Library/Application Support/app.aydy/windows/"` must hold a `<pid>.json` for the new window, and the real `~/Library/Application Support/app.aydy/windows/` must not gain that pid. If the window file appears under the real home instead, quit the isolated instance at once, skip the rest of the check and report it as not run.
  6. Simulate a working session in the scratch home (no real Claude Code session): `ID=check-session-1`; write a transcript `T="$H/.claude/projects/-demo/$ID.jsonl"` with one genuine user prompt line; write the status file `"$H/Library/Application Support/app.aydy/status/$ID.json"` = `{"session_id":"<ID>","hook_event_name":"UserPromptSubmit","cwd":"<S>/folders/demo","transcript_path":"<T>"}` (shape as `extension/hook/aydy-hook.sh` writes it); then `HOME="$H" CLAUDE_CODE_SESSION_ID=$ID plugin/skills/progress/scripts/aydy-progress --eta 6m --step 4/7` (task 03_01's script; `"$H/.claude/sessions"` must not exist, so no live-session filter applies).
  7. Within about 5 s the window's workspace file (`"$S/folders/demo.code-workspace"`, created when the extension reopened the folder as a workspace) has `window.title` starting `◐ 4/7 · ~6m `. Then remove the progress file and `touch -t` the transcript and the status file to 10 minutes ago: the title becomes `◌ quiet 10m …`.
     Cleanup (always, even after a failure): quit the isolated instance only (close its windows, or `pkill -f -- "--user-data-dir $S/data"` — the pattern must contain the scratch path so no other VS Code process matches), confirm `code --list-extensions --show-versions` still equals `"$S/before.txt"` and `~/.claude/settings.json`'s mtime is unchanged, then `rm -rf "$S"`. Report any difference to the owner instead of trying to repair their profile.
