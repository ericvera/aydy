import type { BlocklistValue } from 'unocss'
import { defineConfig } from 'unocss'
import type { PresetStrictDesignTheme } from 'unocss-preset-strict-design'
import { presetStrictDesign } from 'unocss-preset-strict-design'

/**
 * AYDY is dark only (there is no light theme and no theme switching), so the
 * neutral scale is dark-first: `neutral-1` is the popover background and
 * `neutral-7` is the brightest text. This is the inverse of the usual
 * light-first convention.
 */
const theme: PresetStrictDesignTheme = {
  colors: {
    brand: {
      primary: '#0a84ff',
    },

    neutral: {
      // Popover background
      1: '#1c1c1e',
      // Row hover background
      2: '#242427',
      // Dividers, chips
      3: '#2e2e32',
      // Borders
      4: '#3a3a3f',
      // Muted text
      5: '#6e6e78',
      // Secondary text
      6: '#a0a0aa',
      // Primary text
      7: '#ececef',
    },

    /**
     * Status colour is reserved for session status. Unread, selection, and
     * emphasis must never borrow one of these.
     */
    status: {
      working: '#0a84ff',
      'needs-input': '#ff9f0a',
      done: '#30d158',
      error: '#ff453a',
      idle: '#6e6e78',
    },

    // 15% opacity = 26
    statustransparent: {
      'needs-input': '#ff9f0a26',
      done: '#30d15826',
      error: '#ff453a26',
    },
  },

  text: {
    0: { fontSize: '11px', lineHeight: '1.4' },
    1: { fontSize: '12px', lineHeight: '1.4' },
    // Body — the macOS default
    2: { fontSize: '13px', lineHeight: '1.4' },
    3: { fontSize: '15px', lineHeight: '1.35' },
    4: { fontSize: '18px', lineHeight: '1.3' },
  },

  fontWeight: {
    1: '400',
    2: '500',
    3: '600',
  },

  spacing: {
    // Use sparingly only for pixel-based alignment
    '1px': '1px',
    0: '0px',
    1: '2px',
    2: '4px',
    3: '8px',
    4: '12px',
    5: '16px',
    6: '24px',
    7: '32px',
    8: '48px',
  },
}

/**
 * THE `var(--spacing)` TRAP
 * Wind4 resolves every bare-number spacing utility to
 * `calc(var(--spacing) * n)` before it consults `theme.spacing`. That base
 * `--spacing` variable only exists for a linear scale, and this one is
 * non-linear, so the strict preset never defines it and the browser drops
 * every declaration that references it — silently, and with no lint error.
 *
 * The preset itself covers `m-*`, `p-*`, `gap-*`, `w-*`, `h-*`, `min-*` and
 * `max-*`. Every other affected family is handled here: either re-resolved
 * from `theme.spacing` by a rule below, or blocklisted so the failure is a
 * lint error instead of dead CSS.
 */

// top/right/bottom/left plus their logical siblings start/end.
const insetDirectionProperties: Record<string, string> = {
  top: 'top',
  right: 'right',
  bottom: 'bottom',
  left: 'left',
  start: 'inset-inline-start',
  end: 'inset-inline-end',
}

// The inset shorthands, keyed by the optional axis segment.
const insetShorthandProperties: Record<string, string> = {
  '': 'inset',
  x: 'inset-inline',
  y: 'inset-block',
}

// Property suffixes for scroll-margin/scroll-padding, keyed by the direction
// segment of scroll-m*/scroll-p*.
const scrollDirectionSuffixes: Record<string, string[]> = {
  '': [''],
  x: ['-inline'],
  y: ['-block'],
  t: ['-top'],
  r: ['-right'],
  b: ['-bottom'],
  l: ['-left'],
  s: ['-inline-start'],
  e: ['-inline-end'],
}

/**
 * Utilities whose numeric keys reach wind4's `calc(var(--spacing) * n)` path.
 * A key that names a spacing token is served by the rules below; any other
 * number is dead CSS, so it is blocked rather than emitted.
 */
const themedSpacingUtilities =
  /^-?(?:inset(?:-[xy])?|top|right|bottom|left|start|end|space-[xy]|basis|indent|scroll-[mp][xytrblse]?|border-spacing(?:-[xy])?|size(?:-(?:min|max))?|(?:min-|max-)?(?:block|inline))-(.+)$/

/**
 * Same trap, but with no design token behind the utility at all: line height
 * belongs to the `text-*` ramp, and translation is expressed as a percentage
 * (`translate-x-1/2`, `translate-x-full`), which wind4 resolves correctly.
 * Every numeric key in these families is dead CSS.
 */
const untokenizedSpacingUtilities = /^-?(?:leading|translate(?:-[xyz])?)-(.+)$/

const blocksUnthemedNumber =
  (pattern: RegExp, section: Record<string, string>): BlocklistValue =>
  (selector: string): boolean => {
    const key = pattern.exec(selector)?.[1]

    if (key === undefined) {
      return false
    }

    return section[key] === undefined && !Number.isNaN(Number(key))
  }

export default defineConfig({
  presets: [presetStrictDesign({ theme })],

  blocklist: [
    /**
     * The same trap reached from ordinary English. Wind4 answers a bare margin
     * or padding shorthand carrying no value at all — `me`, `ms`, `px`, `pt` —
     * with `calc(var(--spacing) * 4)`, and the extractor reads every word in
     * the interface's copy, so **Show me the change first** puts a dead `.me`
     * rule in the stylesheet. There is no legitimate use: every real spacing
     * utility names a token.
     */
    [/^[mp][xytrbles]$/, { message: 'Spacing utilities name a token.' }],
    [
      blocksUnthemedNumber(themedSpacingUtilities, theme.spacing ?? {}),
      { message: 'Use theme values.' },
    ],
    [
      blocksUnthemedNumber(untokenizedSpacingUtilities, {}),
      {
        message:
          'No theme values. Use text-* for line height and a percentage for translate.',
      },
    ],
  ],

  rules: [
    /**
     * CUSTOM RULES
     * These are rules that are not part of the Tailwind framework but are part
     * of the design system.
     */

    // Single inset directions (top-0, right-5, -bottom-3, start-4, …).
    // Every rule in this group works the same way: resolve the key from the
    // theme spacing scale, and return undefined for anything else so
    // non-spacing keys (auto, full, fractions) fall through to the preset's
    // own rules. Negative forms are handled by wind4's negative variant,
    // which strips the leading dash before matching and negates the value.
    [
      /^(top|right|bottom|left|start|end)-(.+)$/,
      ([, direction, key], { theme }) => {
        if (direction === undefined || key === undefined) {
          return undefined
        }

        const property = insetDirectionProperties[direction]
        const value = theme.spacing?.[key]

        return property === undefined || value === undefined
          ? undefined
          : { [property]: value }
      },
    ],

    // The inset shorthands (inset-0, inset-x-4, -inset-3). inset-0 on an
    // absolutely positioned overlay is the most idiomatic reach of the whole
    // group, and the one that fails most visibly when it emits dead CSS.
    [
      /^inset(?:-([xy]))?-(.+)$/,
      ([, axis, key], { theme }) => {
        if (key === undefined) {
          return undefined
        }

        const property = insetShorthandProperties[axis ?? '']
        const value = theme.spacing?.[key]

        return property === undefined || value === undefined
          ? undefined
          : { [property]: value }
      },
    ],

    // Flex basis (basis-0 on a growing row column).
    [
      /^basis-(.+)$/,
      ([, key], { theme }) => {
        if (key === undefined) {
          return undefined
        }

        const value = theme.spacing?.[key]

        return value === undefined ? undefined : { 'flex-basis': value }
      },
    ],

    // Text indent.
    [
      /^indent-(.+)$/,
      ([, key], { theme }) => {
        if (key === undefined) {
          return undefined
        }

        const value = theme.spacing?.[key]

        return value === undefined ? undefined : { 'text-indent': value }
      },
    ],

    // Scroll margin and scroll padding, which a scrollable session list needs
    // to keep a keyboard-selected row clear of the list edge.
    [
      /^scroll-([mp])([xytrblse]?)-(.+)$/,
      ([, box, direction, key], { theme }) => {
        if (box === undefined || direction === undefined || key === undefined) {
          return undefined
        }

        const suffixes = scrollDirectionSuffixes[direction]
        const value = theme.spacing?.[key]

        if (suffixes === undefined || value === undefined) {
          return undefined
        }

        const property = box === 'm' ? 'scroll-margin' : 'scroll-padding'

        return Object.fromEntries(
          suffixes.map((suffix) => [`${property}${suffix}`, value]),
        )
      },
    ],

    // The space-x/space-y stack utilities, emitted in the classic
    // sibling-margin form.
    [
      /^space-([xy])-(.+)$/,
      ([, axis, key], { theme, symbols }) => {
        if (axis === undefined || key === undefined) {
          return undefined
        }

        const value = theme.spacing?.[key]

        if (value === undefined) {
          return undefined
        }

        return {
          [symbols.selector]: (selector: string) =>
            `${selector} > :not(:last-child)`,
          [axis === 'y' ? 'margin-bottom' : 'margin-inline-end']: value,
        }
      },
    ],

    // Shared divider between stacked session rows: a 1px neutral-3 line after
    // every row except the last. Selector-based (:not(:last-of-type)), which
    // plain utilities can't express — one rule so every list shares a single
    // divider definition. Never re-implement it as scoped CSS.
    [
      /^row-divider$/,
      (_, { theme, symbols }) => {
        const neutral = theme.colors?.['neutral']
        const color = typeof neutral === 'object' ? neutral[3] : undefined

        if (typeof color !== 'string') {
          return undefined
        }

        return {
          [symbols.selector]: (selector: string) =>
            `${selector}:not(:last-of-type)`,
          'border-bottom-width': '1px',
          'border-bottom-style': 'solid',
          'border-bottom-color': color,
        }
      },
    ],

    // The line between stacked session groups, the same 1px neutral-3 rule
    // seen the other way up: a top border on every group that follows another,
    // which the adjacent-sibling combinator expresses and plain utilities
    // cannot. Unrelated to the `group` marker class that `group-hover:` reads.
    [
      /^group-divider$/,
      (_, { theme, symbols }) => {
        const neutral = theme.colors?.['neutral']
        const color = typeof neutral === 'object' ? neutral[3] : undefined

        if (typeof color !== 'string') {
          return undefined
        }

        return {
          [symbols.selector]: (selector: string) => `${selector} + ${selector}`,
          'border-top-width': '1px',
          'border-top-style': 'solid',
          'border-top-color': color,
        }
      },
    ],

    // Rounded corners. `rounded-sm` cannot be used as a name — the strict
    // preset blocks every `-sm`/`-lg`-style size suffix on utilities with no
    // theme section behind them.
    ['rounded', { 'border-radius': '6px' }],
    ['rounded-small', { 'border-radius': '4px' }],
    // The popover shell's own corner, straight from the mock.
    ['rounded-large', { 'border-radius': '10px' }],

    // The popover is a fixed 320px wide. A token because the strict preset
    // blocks bracket values like w-[320px] and 320px is not a spacing step.
    ['w-popover', { width: '320px' }],
  ],

  shortcuts: {
    /**
     * CUSTOM SHORTCUTS
     * These are shortcuts that are not part of the Tailwind framework but are
     * part of the design system.
     */

    // Unread is carried by weight and brightness, never colour — the status
    // dot owns colour. Use these two so no component invents its own pair.
    'title-unread': 'font-3 text-neutral-7',
    'title-read': 'font-1 text-neutral-6',
  },
})
