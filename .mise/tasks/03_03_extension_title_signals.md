# 03_03 Extension: work signal in the window title

## Goal

Each VS Code window's title shows its most urgent working session's signal — `◐ 4/7 · ~6m`, `◐ 2/5`, `◐ 3m`, or `◌ quiet 9m` — computed by the window tracker on every pass (variant B of the approved mock).

## Files to modify/create

- `extension/src/windowTracker.ts`
- `extension/src/extension.ts`
- `extension/README.md`

## Background

The AYDY Companion extension keeps each window's title through its generated `.code-workspace` file: `WindowTracker` (`extension/src/windowTracker.ts`) reads the status/seen/window files on every change and every 5 s (`TRACKER_POLL_MS`), works out which sessions this window owns (`pass`, `:213-247`), and calls `onGlance` when the glance changed; `extension.ts` (`activate`, `:494-541`; `writeTitle`, `:103-114`) turns the glance into a title with `tabTitle` and writes it with `setWorkspaceTitle` (which skips the write when the title is unchanged).

Task 03_02 produced, in `extension/src/workSignal.ts`: `sessionSignal(activity, now)`, `mostUrgent(signals)`, `titleSignal(signal)`, `SessionActivity`, `ProgressReport`; in `extension/src/sessionStore.ts`: `readProgress(dirs)`, `transcriptActivity(transcriptPath, sessionId, cache)`; in `extension/src/sessionState.ts`: `SessionStatus.transcriptPath`, `WindowGlance.signal: TitleSignal | null`, `windowGlance(statuses, seen, signal)`, and `tabTitle` already rendering a signal; in `extension/src/aydyPaths.ts`: `progressDir()`.

Owner decisions: the title shows the most urgent working session — stale first, then the shortest ETA (`mostUrgent` encodes the full order); unread still takes the slot (`● 1 …`), exactly as today. Mock: `.mise/mock/b-vscode-title.html`.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `windowTracker.ts`:
   - `TrackerDirs` gains `progress`; `defaultDirs` uses `progressDir()`.
   - `TrackerStore` gains `readProgress: () => Promise<Map<string, ProgressReport>>` and `transcriptActivity: (transcriptPath: string, sessionId: string) => Promise<{ writtenAt: number | null; checklist: Step | null }>`; `defaultStore` binds them (the tail cache is a `Map` owned by the default store closure).
   - `start()` watches the progress dir along with the others (create it with `mkdir -p` like them).
   - In `pass()`, after ownership: for each owned status where `isWorking(status)`, build a `SessionActivity` (`transcriptActivity` only when `transcriptPath !== null`, else nulls; progress from the map), compute `sessionSignal(activity, Date.now())`, then `titleSignal(mostUrgent(signals))` (null-safe), and pass it as the third argument to `windowGlance`.
   - The change check compares `unread`, `working`, and the signal (`stale` and `text`), so a title changes when its text changes and not otherwise.
   - Read failures for one session's transcript cost that session's activity only (catch per session), never the pass.
2. `extension.ts`: no logic change beyond what the compiler needs — `writeTitle` already uses `tabTitle(glance, …)`; confirm the initial `glance` literal carries `signal: null` (added in 03_02).
3. `extension/README.md`: in "What it does", describe the title signals (glyphs, what each text means, stale = nothing written by the session or its subagents for 5 minutes, the most-urgent rule, that progress comes from the `aydy:progress` plugin skill in `plugin/`, then TodoWrite, then elapsed time), and add `progress/` to the list of files under `app.aydy/` it reads and prunes.

## Gotchas

- Title texts are minute-granular by design (03_02), so with a 5 s poll the workspace file is rewritten at most about once a minute per changing session; do not add a faster poll.
- Keep the pass bounded: a stat walk and at most one tail read per owned working session per pass, and the tail read only when the transcript changed (the store's cache).
- `windowTracker.test.ts` injects a fake `store` and `watchDir`; extend the fake with the two new methods and keep existing tests green.
- Do not compute signals for sessions this window does not own, or for non-working sessions.

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests in `windowTracker.test.ts` (fake store, fake clock via `vi.useFakeTimers()` / `vi.setSystemTime` as existing tests do):
  - a working owned session with an accepted progress report produces a glance whose signal text is `4/7 · ~6m`;
  - a working owned session whose transcript activity is 9 minutes old produces `{ stale: true, text: 'quiet 9m' }`;
  - two working sessions, one stale and one with an ETA, → the stale one wins; two with ETAs → the shorter;
  - a session owned by another window contributes nothing;
  - `onGlance` is not called again when nothing changed, and is called when only the signal text changed;
  - a transcript read that throws for one session leaves the other session's signal intact.
- Substitute check: package and install the extension (`yarn package-extension`, then `code --install-extension extension/aydy-companion-0.1.0.vsix`), start a Claude Code session in a workspace window, run the plugin command from 03_01 by hand with `CLAUDE_CODE_SESSION_ID` set to that session, and see the title change; close any VS Code windows and scratch folders you opened afterwards.
