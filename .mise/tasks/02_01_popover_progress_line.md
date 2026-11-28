# 02_01 Popover progress line and hollow stale dot

## Goal

Every Working row in the popover gains an always-visible second line showing progress (bar plus text) or **Quiet Nm**, and a stale session's dot becomes a hollow ring — variant A from the approved mock.

## Files to modify/create

- `src/types/WorkSignals.ts` (new)
- `src/types/Session.ts`
- `src/components/atoms/StatusDot.vue`
- `src/components/molecules/SessionProgressLine.vue` (new)
- `src/components/molecules/SessionRow.vue`
- `docs/design/ui-conventions.md` (one sentence; docs do not count toward the limit)

## Background

AYDY's popover is Vue 3 + TypeScript + UnoCSS with a strict design preset, dark only (`src/`). It renders exactly what Rust sends and derives nothing about state (`docs/design/menubar-ux.md`, "Where the numbers come from"). Rust now sends, on every session, `signals` (task 01_05, `src-tauri/src/ipc/snapshot_channel.rs` `SignalsPayload`), `null` unless the session is Working:

```ts
{
  stale: boolean
  quietMs: number | null
  turnElapsedMs: number | null
  step: { done: number; total: number; source: 'agent' | 'checklist' } | null
  eta: { remainingMs: number; reportedAgoMs: number } | null   // remainingMs < 0 = past the ETA
  fractionPermille: number | null                              // 0–1000
}
```

The approved design is `.mise/mock/a-row-line.html` (rendered by `.mise/mock/mock.js` `row()` / `progressText()` and `.mise/mock/mock.css` `.meta`, `.bar`, `.dot.stale`). Open the mock HTML in a browser or read the JS/CSS. The owner chose variant A knowing it breaks the one-line-row rule for Working rows only; Done, Needs input and Unknown rows stay one line.

Line content (mock `row()` for variant A):

- **Not stale:** `[bar] <progress text>`; the bar only when `fractionPermille !== null`.
- **Stale:** `**Quiet <formatDuration(quietMs)>**` (brighter, `text-neutral-6`) then `<progress text>`; no bar.
- **Progress text:** parts joined by `·`: `${done}/${total}` when `step`; `~${formatDuration(remainingMs)} left` when `eta.remainingMs >= 0`, `${formatDuration(-remainingMs)} past ETA` when negative; when neither step nor eta, `${formatDuration(turnElapsedMs)} elapsed` (omit the part entirely if `turnElapsedMs` is null — then a not-stale line with nothing to say is not rendered).

Existing patterns: `SessionRow.vue` (row layout `flex-col gap-1 px-5 py-2`, title line `flex items-center gap-3`), `SessionRowDetail.vue` (meta text `text-0 text-neutral-5`), `src/utils/formatDuration.ts` (`1h 12m`, `6m`, `45s`), `StatusDot.vue` (status colour map), tests `src/components/molecules/SessionRow.test.ts` with `aSession` (`src/__test__/aSession.ts`) and `renderedText`.

## Guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `src/types/WorkSignals.ts`: `export interface WorkSignals { … }` exactly the shape above, with a doc comment (durations are whole ms; null unless Working). One export per file.
2. `src/types/Session.ts`: add `signals: WorkSignals | null`. Update `src/__test__/aSession.ts` default `signals: null` (test support).
3. `StatusDot.vue`: new optional prop `stale?: boolean` (default false). When `stale` is true, render a hollow ring in `status-working`: no background fill, `border border-status-working` (the dot keeps `h-3 w-3 shrink-0 rounded-full`; use `box-border` if needed so it stays 8px). Comment: the dot still owns the colour; stale keeps the working hue because status stays Working. Stale only ever arrives with status `working`.
4. `SessionProgressLine.vue` (molecule, props `signals: WorkSignals`): `<span class="flex items-center gap-3 pl-5 text-0 text-neutral-5">` — `pl-5` (16px) lines the text up under the title (8px dot + 8px gap). Bar: track `<span class="h-1 w-8 shrink-0 overflow-hidden rounded-full bg-neutral-3">` (48px × 2px) with a fill `<span class="block h-full bg-neutral-6">` whose width is the one dynamic inline style, `:style="{ width: `${fractionPermille / 10}%` }"`, with a comment that a runtime fraction is the one thing utilities cannot express. Text per the rules above; build the text in a `computed`, not in the template.
5. `docs/design/ui-conventions.md`, under "SFC idiom": one sentence that a value known only at runtime (the progress bar's fill width) is the one place a bound inline `style` is used, because no utility can express it; everything static stays a utility.
6. `SessionRow.vue`: pass `:stale="props.session.signals?.stale ?? false"` to `StatusDot`; render `<SessionProgressLine v-if="showLine" :signals="props.session.signals" />` between the title line and `SessionRowDetail`, where `showLine` = status is `working`, `signals !== null`, and the line has something to say (stale, step, eta, or a non-null `turnElapsedMs`).

## Gotchas

- Read `docs/design/ui-conventions.md` first. Spacing is non-linear (`w-8` = 48px, `h-1` = 2px, `pl-5` = 16px, `gap-3` = 8px); unknown tokens silently emit nothing. After building, check the generated CSS really contains rules for `border-status-working`, `w-8`, `h-1`, `pl-5`, `bg-neutral-3`, `bg-neutral-6`; if any new family is used, add it to the relevant sweep in `uno.config.test.ts`.
- Status colour is reserved for the dot: the bar fill and **Quiet** text use neutrals, never a status colour.
- The line is always visible on Working rows — it is not part of the hover reveal and gets no `group-hover` classes. Do not touch the hover detail's reveal classes (that is the anti-flicker mechanism, menubar-ux.md "The reveal moves nothing").
- `SessionRow.test.ts` snapshots the whole row for a Done session (`shows a status dot and a title and nothing else`); it must stay unchanged, which proves non-Working rows are still one line. Its test name and doc comment should be adjusted to say "a row that is not working".
- `lint` runs eslint + stylelint; no `<style>` block is needed.
- The detail line's own content is task 02_02; leave `SessionRowDetail` alone here.

## Verification

- Run `yarn build-tsc`, `yarn lint`, `yarn test`.
- New/updated tests:
  - `StatusDot.test.ts`: stale working dot renders the hollow ring classes and no `bg-status-working`; non-stale unchanged (existing inline snapshots stay green).
  - `SessionProgressLine.test.ts` (new): `4/7 · ~6m left` with a bar at the fraction; `2/5` alone; `~6m left` alone; `2m past ETA`; `3m elapsed` with no bar when there is neither step, eta nor fraction; stale renders `Quiet 9m` first, no bar, then the progress text.
  - `SessionRow.test.ts`: a Done/needsInput row is unchanged (existing snapshot); a working row with signals renders the progress line; a working row with `signals: null` renders no line; a stale working row passes `stale` to the dot.
- Substitute visual check: run the popover UI in a browser (`yarn dev`, or mount the components) at 320px and compare against `.mise/mock/a-row-line.html` "At rest" and "Past its ETA".
