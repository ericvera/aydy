# 02_02 Hover detail gains the work signals

## Goal

A Working row's hover detail explains its signals: what quiet means when stale, how long this turn has run, and how long ago the agent reported its ETA (or that it has not). Record the new row behaviour in the design doc.

## Files to modify/create

- `src/components/molecules/SessionRowDetail.vue`
- `src/components/molecules/SessionRow.vue`
- `docs/design/menubar-ux.md`

## Background

AYDY's popover (Vue 3 + UnoCSS strict preset, `src/`) reveals a detail line under each row on hover with pure CSS (`SessionRowDetail.vue`, its long doc comment explains the anti-flicker `group/list` + `group/row` mechanism — do not change the reveal classes). Today it shows `Status · Working time X · Longest turn Y`.

Task 02_01 added `Session.signals: WorkSignals | null` (`src/types/WorkSignals.ts`: `stale`, `quietMs`, `turnElapsedMs`, `step {done,total}` (only from an agent report), `eta {remainingMs, reportedAgoMs}`, `fractionPermille`; null unless Working), the always-visible `SessionProgressLine` under Working rows, and the hollow stale dot.

Approved design (variant A of a mock that is not part of the repository; its two hover frames are inlined here). With the pointer on a working row the detail line read `Working | This turn 12m | ETA reported 1m ago | Working time 38m | Longest turn 14m`; on the stale row it read `Working | Quiet 9m · nothing written to the transcript | This turn 31m | No ETA reported | Working time 52m | Longest turn 14m` (the mock's stale row read `No ETA reported · checklist only`; the owner dropped the checklist fallback, goals decision 10, so it now reads `No ETA reported`; `|` marks part boundaries for reading only and is not rendered; the `·` inside a part is literal copy; in the UI each part is its own `<span>` in the `gap-3` flex row, 11px `neutral-5`, the quiet part one step brighter in `neutral-6`). Order of parts for a Working row with signals:

1. status text (`Working`) — existing;
2. when stale: `Quiet <quiet> · nothing written to the transcript` (rendered `text-neutral-6`, a neutral — never a status colour);
3. `This turn <turnElapsed>` (omit if `turnElapsedMs` is null);
4. `ETA reported <reportedAgo> ago` when `eta`, else `No ETA reported`;
5. `Working time …`, `Longest turn …` — existing.

Durations via `src/utils/formatDuration.ts`. Rows that are not Working, or have `signals: null`, render exactly as today.

## Guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `SessionRowDetail.vue`: new prop `signals: WorkSignals | null`. Build the list of extra parts in a `computed` (only when `status === 'working'` and `signals !== null`) and render them between the status text and `Working time`, each in its own `<span>` like the existing parts (the container is `flex-wrap gap-3`). Keep the root element's classes byte-identical.
2. `SessionRow.vue`: pass `:signals="props.session.signals"` to `SessionRowDetail`.
3. `docs/design/menubar-ux.md`:
   - Line 128, the popover layout item **Body** — `the grouped session list, one line per session.`: rewrite so it no longer claims one line per session, e.g. `the grouped session list, one line per session, plus a progress line under each Working row.`
   - Section "The list":
   - Rewrite the "A row is a status dot and a title, on one line" bullet: still true for every row except Working rows, which carry a second, always-visible progress line: a bar and `4/7 · ~6m left` / `~6m left` / `2m past ETA` / `3m elapsed`, or **Quiet 9m** first when stale. Say why it is worth breaking the rule (the owner wanted progress and liveness without opening anything) and that it costs height only for working sessions.
   - Add a bullet for the hollow dot: a stale session's dot is a hollow ring in working blue; status stays Working; stale is a signal beside status, set when nothing has been written to the session's transcript or its subagents' transcripts for 5 minutes.
   - Add where progress comes from: only the agent's own report through the `aydy:progress` skill (an ETA from before the current prompt is discarded); without one the line shows the turn's elapsed time. AYDY does not read TodoWrite or task lists.
   - Update the "Hover detail" bullet to list the new parts.
   - Update "Where the numbers come from" to mention the progress files (`progress/`, written by the plugin's `aydy-progress`) and the transcript activity read on the 25 s scan.

## Gotchas

- The detail is revealed only on hover; the new parts are hover-only and must not duplicate into the always-visible line beyond what 02_01 shows.
- Existing expectations for detail text `'Working Working time 1h 12m Longest turn 40m'` with `signals: null` (`src/components/molecules/SessionRowDetail.test.ts:25` and `src/components/molecules/SessionRow.test.ts:92`) must stay green.
- Copy is English; emphasise UI terms with bold in docs, never quotes (ui-conventions.md, Typography).

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New tests in `SessionRowDetail.test.ts`:
  - working, not stale, with ETA: `Working This turn 12m ETA reported 1m ago Working time 38m Longest turn 14m`;
  - working, stale, no report: includes `Quiet 9m · nothing written to the transcript` right after `Working`, then `This turn 31m No ETA reported …`;
  - working, not stale, no report: `Working This turn 3m No ETA reported Working time …`;
  - done row with `signals: null`: unchanged text;
  - the root element's classes are unchanged (reveal mechanism intact).
- `SessionRow.test.ts`: a working row passes its signals through to the detail.
