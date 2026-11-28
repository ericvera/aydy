# UI Conventions

Read this before writing or editing any Vue component or view in `src/`.

AYDY is a macOS menu bar app whose entire UI is one compact popover. It is
**dark only** — there is no light theme and no theme switching. Do not add one
"for completeness"; a second theme doubles every visual check.

## The strict preset

The UnoCSS preset is `unocss-preset-strict-design`. **Only the tokens, rules,
and shortcuts below exist.**

- Arbitrary values are blocked: `p-[13px]`, `w-[320px]`, `text-[13px]` all fail
  `yarn lint` through `unocss/blocklist`.
- Size suffixes with no theme section behind them are blocked: `rounded-sm`,
  `shadow-2xs`, `max-w-lg`.
- An unknown token generates **no CSS, with no error** — `text-9` or
  `bg-neutral-8` simply produce nothing. Never guess a class from Tailwind
  memory.
- A spacing utility can be worse than silent: it can emit CSS the browser
  throws away. Read [The `var(--spacing)` trap](#the-var--spacing-trap) before
  reaching for one outside the families listed there.

Source of truth: [uno.config.ts](../../uno.config.ts). When a value you need is
not there, add a token — do not reach for an arbitrary value or scoped CSS.

## Spacing — diverges from Tailwind, the number one trap

Sizing utilities (`w-*`, `h-*`, `min-*`, `max-*`, `gap-*`) resolve from the
same spacing scale. There is no separate width or height section.

| Token | px  | Token | px  |
| ----- | --- | ----- | --- |
| `1px` | 1   | 5     | 16  |
| 0     | 0   | 6     | 24  |
| 1     | 2   | 7     | 32  |
| 2     | 4   | 8     | 48  |
| 3     | 8   |       |     |
| 4     | 12  |       |     |

Tailwind muscle memory says `p-4` is 16px and `p-5` is 20px — here they are
12px and 16px. Common rhythm: `px-5` for the popover gutter, `py-3` for row
height, `gap-3` between a status dot and its label.

`1px` exists only for hairline alignment. Use it for borders, never for
spacing.

## Typography

| Token    | Size | Use                                      |
| -------- | ---- | ---------------------------------------- |
| `text-0` | 11px | meta lines, timestamps, badges           |
| `text-1` | 12px | secondary labels, footer, banners        |
| `text-2` | 13px | **body** — the macOS default             |
| `text-3` | 15px | rare, a single prominent line            |
| `text-4` | 18px | reserved; nothing in the popover uses it |

Three font weights, and only three:

- `font-1` — 400 regular
- `font-2` — 500 medium
- `font-3` — 600 semibold

There is no `font-medium`, `font-semibold`, or `font-bold`.

Prefer typography atoms over raw tags plus classes. A row title is a
`SessionTitle` atom, not `<span class="text-2 font-3 text-neutral-7">` repeated
in five places. When the same class trio appears twice, it wants an atom.

**Emphasise a UI term or button name with bold, never quote characters.** Write
Press **Jump to session** — never wrap the name in quotes, `«»`, or `""`.

## Colors

### Neutrals are dark-first

`neutral-1` is the **background** and `neutral-7` is the **brightest text** —
the reverse of the light-first convention most references (and most other
projects) assume. Copying a light-first mental model here produces a
light-on-light UI.

| Token       | Hex       | Role                       |
| ----------- | --------- | -------------------------- |
| `neutral-1` | `#1c1c1e` | popover background         |
| `neutral-2` | `#242427` | row hover background       |
| `neutral-3` | `#2e2e32` | dividers, chip backgrounds |
| `neutral-4` | `#3a3a3f` | borders                    |
| `neutral-5` | `#6e6e78` | muted text, section labels |
| `neutral-6` | `#a0a0aa` | secondary text             |
| `neutral-7` | `#ececef` | primary text               |

No `white`, `black`, or `gray-500` — use the neutral scale.

### Brand

`brand-primary` (`#0a84ff`) is the only brand color, used for primary buttons.

### Status

| Token                | Hex       | Meaning             |
| -------------------- | --------- | ------------------- |
| `status-working`     | `#0a84ff` | Claude is working   |
| `status-needs-input` | `#ff9f0a` | waiting on the user |
| `status-done`        | `#30d158` | finished            |
| `status-error`       | `#ff453a` | failed              |
| `status-idle`        | `#6e6e78` | no recent activity  |

`statustransparent-needs-input`, `statustransparent-done`, and
`statustransparent-error` are the same hues at 15% for tinted banner
backgrounds.

**Status color is reserved for status.** The status dot owns color, and only
the dot and the text that names the status may use one. Unread, selection,
hover, and emphasis must never borrow a status color — if they did, the dot
would stop being a reliable read of session state.

### Unread is weight and brightness, never color

Unread and read titles are two shortcuts, so no component invents its own pair:

- `title-unread` — `font-3` at `neutral-7`; sits forward.
- `title-read` — `font-1` at `neutral-6`; recedes.

## Custom rules and shortcuts

Use these; do not rebuild them.

- `rounded` — 6px. `rounded-small` — 4px. `rounded-large` — 10px, the popover
  shell's own corner and nothing else's. There is no `rounded-md`/`rounded-lg`,
  and `rounded-sm` is blocked.
- `w-popover` — the popover's fixed 320px width.
- `row-divider` — the shared 1px `neutral-3` bottom border between stacked
  session rows; it skips the last row via `:not(:last-of-type)`. Never
  re-implement it as scoped `:not(:last-of-type)` CSS in a component.
- `group-divider` — the same 1px `neutral-3` line the other way up, between
  stacked session groups; it skips the first group via the adjacent-sibling
  combinator. Unrelated to the `group` marker class below.
- `title-unread` / `title-read` — see above.

## Hover is the browser's, never state

**A reveal on hover is `group` on the container and `group-hover:` on what it
reveals — never a `ref` toggled by `@mouseenter`/`@mouseleave`.** A stored flag
is only ever corrected by a later pointer event over the same element, and the
popover has two situations that produce none: the window is hidden with the
pointer over a row and its DOM stays mounted, and the list reorders under a
still pointer up to ten times a second. Either leaves an element revealed with
the pointer nowhere near it. `:hover` is re-resolved by the engine after a
hide, a show and every reflow, so it cannot go stale.

The revealed element stays in the DOM and is held back by a resting utility the
variant overrides. Give it no unprefixed utility that could win instead:
`hidden` and `flex` have the same specificity, so which one applied would depend
on the order the two rules happened to be generated in, while
`.group:hover .group-hover\:flex` outranks `.hidden` outright.

**A reveal that changes an element's display changes its container's size, so
in a list it must not be scoped to the row.** Revealing a row's own detail line
with `hidden … group-hover:flex` grew that row by the line's height and pushed
every row below it down, and running the pointer down the list was one jump per
row boundary — the reported flicker. Two nested named groups is the way out:
`group/list` on the list and `group/row` on the row, with
`hidden invisible group-hover/list:flex group-hover/row:visible` on the thing
revealed. The pointer being anywhere on the list gives every one of them its
space; the row under the pointer is the only one that paints. Reserve from the
container, paint from the element — and the container must be one whose own box
does not depend on its contents, or the reveal can resize the thing it is
scoped to.

## The `var(--spacing)` trap

UnoCSS's wind4 engine resolves a bare number in a spacing utility to
`calc(var(--spacing) * n)` **before** it looks at the theme. That base
`--spacing` variable only exists for a linear scale; ours is non-linear, so the
strict preset never defines it. **Any utility family that resolves from the
absent `--spacing` base emits CSS the browser drops — silently, with no lint
error and no visible difference from a class that was never written.** It is
the hardest failure in this codebase to diagnose, because the class looks
right, the stylesheet contains a matching rule, and nothing warns.

`uno.config.ts` closes the trap in two ways, and you must not assume a family
is covered just because it looks like one that is:

- **Re-resolved from `theme.spacing`** — these work, including their negative
  forms, and must not be replaced with scoped CSS: `m-*`/`p-*`, `gap-*`,
  `w-*`/`h-*`/`size-*`/`min-*`/`max-*`, `inset-*`/`inset-x-*`/`inset-y-*`,
  `top-*`/`right-*`/`bottom-*`/`left-*`/`start-*`/`end-*`, `space-x-*`/
  `space-y-*`, `basis-*`, `indent-*`, `scroll-m*-*`/`scroll-p*-*`.
- **Blocked, so `yarn lint` reports them** — a number outside the spacing scale
  (`top-9`, `inset-9`, `size-9`), and every numeric `leading-*` and
  `translate-*`. Line height belongs to the `text-*` ramp, and translation is
  expressed as a percentage: `translate-x-1/2` and `translate-x-full` resolve
  correctly and are the supported forms.
- **Blocked because English spells them** — the bare margin and padding
  shorthands `me`, `ms`, `mt`, `mb`, `ml`, `mr`, `mx`, `my` and their `p`
  siblings. The extractor reads every word in the file, not only class
  attributes, so the copy **Show me the change first** generated a dead
  `.me{margin-inline-end:calc(var(--spacing) * 4)}` rule until they were
  blocked. Nothing carried that class; the rule was simply noise in the
  bundle — and the bundle containing no `var(--spacing)` is the only detector
  this codebase has for the trap, so it is kept true.

Non-spacing keys — `inset-auto`, `top-full`, `basis-full`, `leading-none` —
fall through to the preset and work normally.

If you need a spacing utility from a family not listed above, do not just use
it. Generate the CSS first and check for `var(--spacing)` in the output; if it
leaks, add a rule in `uno.config.ts` and an entry to the sweep in
`uno.config.test.ts`, which asserts that no enumerated utility leaks.

## Base styles and the layering trap

`src/main.css` holds the popover base — system font stack, 13px body, dark
background. It lives in `@layer main`, while **UnoCSS emits its reset and its
utilities unlayered**, and unlayered CSS beats every layer. That is deliberate:
a utility must always win over a base style. The consequence is that a
declaration in `main.css` which collides with the UnoCSS reset silently does
nothing. Never derive a spacing decision from `main.css` — measure the computed
value.

## Components

### Layering (`src/components/`)

- **atoms/** — a single control or typography primitive. Props and emits only,
  no stores, no data fetching.
- **molecules/** — small compositions of atoms, still prop-driven (a session
  row, a status dot with its label).
- **organisms/** — feature-level compositions that may hold richer local state
  (the popover body, a settings panel).

Views stay thin: a view wires an organism to state and nothing else.
Coordination — polling, sorting, selection, keyboard handling — lives in a
composable (`usePopover`, `useSessions`), not in the view.

### SFC idiom

```vue
<script setup lang="ts">
interface Props {
  id: string
  title: string
  unread: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{ select: [id: string] }>()
</script>

<template>
  <button
    class="row-divider flex items-center gap-3 px-5 py-3"
    @click="emit('select', props.id)"
  >
    <span :class="props.unread ? 'title-unread' : 'title-read'">
      {{ props.title }}
    </span>
  </button>
</template>
```

- `<script setup lang="ts">` with an `interface Props` and `defineProps<Props>()`
  — never inline object props.
- `defineEmits` is always typed.
- `<style scoped>` only for what utilities genuinely cannot express. Check the
  custom rules above first — the strictness applies to theme tokens, not to
  structural CSS. Standard utilities like `relative`, `overflow-hidden`,
  `flex`, `truncate`, `whitespace-nowrap` all exist.
- A value known only at runtime — the progress bar's fill width — is the one
  place a bound inline `style` is used, because no utility can express it;
  everything static stays a utility.
- Comments go above the code they describe, never inline, and wrap at 80
  characters.

### One export per file

Every file has exactly one export and the filename matches it:
`SessionRow.vue` exports `SessionRow`, `useSessions.ts` exports `useSessions`.
