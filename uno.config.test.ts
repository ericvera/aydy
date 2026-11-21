import { createGenerator } from 'unocss'
import { expect, it } from 'vitest'

import config from './uno.config'

const generate = async (classes: string): Promise<string> => {
  const uno = await createGenerator(config)
  const { css } = await uno.generate(classes)

  return css
}

const blocked = async (classes: string): Promise<string[]> => {
  const uno = await createGenerator(config)
  await uno.generate(classes)

  return [...uno.blocked]
}

it('generates CSS for one utility from every theme section', async () => {
  const css = await generate('p-4 gap-3 w-5 text-2 font-3 text-neutral-7')

  expect(css).toContain('.p-4{padding:12px;}')
  expect(css).toContain('.gap-3{gap:8px;}')
  expect(css).toContain('--spacing-5: 16px')
  expect(css).toContain('.w-5{width:var(--spacing-5);}')
  expect(css).toContain('--text-2-fontSize: 13px')
  expect(css).toContain('--fontWeight-3: 600')
  expect(css).toContain('--colors-neutral-7: #ececef')
})

it('generates CSS for the status and tinted status colors', async () => {
  const css = await generate(
    'text-status-needs-input bg-statustransparent-error text-brand-primary',
  )

  expect(css).toContain('--colors-status-needs-input: #ff9f0a')
  expect(css).toContain('--colors-statustransparent-error: #ff453a26')
  expect(css).toContain('--colors-brand-primary: #0a84ff')
  expect(css).toContain('.bg-statustransparent-error{background-color:')
})

/**
 * Every utility family whose bare-number key wind4 resolves through
 * `calc(var(--spacing) * n)`. The strict preset defines no base `--spacing`,
 * so any family missing from `uno.config.ts` emits CSS the browser drops —
 * silently, and with no lint error. This list is the guard: a family added to
 * wind4, or one overlooked here, fails the sweep below instead of shipping.
 */
const spacingUtilities = [
  'inset-0',
  'inset-3',
  '-inset-3',
  'inset-x-4',
  'inset-y-4',
  '-inset-x-4',
  'top-0',
  '-top-3',
  'right-5',
  'bottom-2',
  'left-4',
  'start-3',
  '-start-3',
  'end-3',
  'space-x-3',
  'space-y-4',
  '-space-x-3',
  'basis-0',
  'basis-4',
  'indent-3',
  '-indent-3',
  'scroll-m-3',
  'scroll-mt-3',
  'scroll-mx-4',
  'scroll-p-3',
  'scroll-pb-2',
  'scroll-ps-3',
  'm-3',
  '-m-3',
  'mx-4',
  'p-3',
  'px-5',
  'gap-3',
  'gap-x-4',
  'w-5',
  'h-3',
  'size-3',
  'min-w-4',
  'max-h-6',
  'border-spacing-3',
]

it('never leaks the undefined --spacing base', async () => {
  const uno = await createGenerator(config)
  const { css } = await uno.generate(spacingUtilities.join(' '))

  expect(css).not.toContain('var(--spacing)')
  // Without this a blocked utility would pass the assertion above by
  // generating nothing at all.
  expect([...uno.blocked]).toStrictEqual([])
})

it('emits real values for the inset, stack and scroll utilities', async () => {
  const css = await generate(
    'inset-0 inset-3 -inset-3 inset-x-4 inset-y-4 top-0 left-4 -top-3 start-3 end-3 space-y-4 space-x-3 basis-0 indent-3 scroll-mt-3 scroll-p-4',
  )

  expect(css).toContain('.inset-0{inset:0px;}')
  expect(css).toContain('.inset-3{inset:8px;}')
  expect(css).toContain('.-inset-3{inset:-8px;}')
  expect(css).toContain('.inset-x-4{inset-inline:12px;}')
  expect(css).toContain('.inset-y-4{inset-block:12px;}')
  expect(css).toContain('.top-0{top:0px;}')
  expect(css).toContain('.left-4{left:12px;}')
  expect(css).toContain('.-top-3{top:-8px;}')
  expect(css).toContain('.start-3{inset-inline-start:8px;}')
  expect(css).toContain('.end-3{inset-inline-end:8px;}')
  expect(css).toContain('.space-y-4 > :not(:last-child){margin-bottom:12px;}')
  expect(css).toContain(
    '.space-x-3 > :not(:last-child){margin-inline-end:8px;}',
  )
  expect(css).toContain('.basis-0{flex-basis:0px;}')
  expect(css).toContain('.indent-3{text-indent:8px;}')
  expect(css).toContain('.scroll-mt-3{scroll-margin-top:8px;}')
  expect(css).toContain('.scroll-p-4{scroll-padding:12px;}')
})

it('falls through to the preset for non-spacing keys', async () => {
  const css = await generate(
    'inset-auto inset-1/2 top-full top-auto basis-full translate-x-1/2 leading-none',
  )

  expect(css).toContain('.inset-auto{inset:auto;}')
  expect(css).toContain('.top-full{top:100%;}')
  expect(css).toContain('.top-auto{top:auto;}')
  expect(css).toContain('.basis-full{flex-basis:100%;}')
  expect(css).toContain('--un-translate-x:50%')
  expect(css).toContain('--leading-none: 1')
})

/**
 * Line height comes from the `text-*` ramp and translation from percentages,
 * so these families have no spacing token behind them at all. Blocking them
 * turns dead CSS into a lint error.
 */
it('blocks numeric keys that no spacing token can serve', async () => {
  expect(await blocked('inset-9')).toStrictEqual(['inset-9'])
  expect(await blocked('top-9')).toStrictEqual(['top-9'])
  expect(await blocked('size-9')).toStrictEqual(['size-9'])
  expect(await blocked('space-x-9')).toStrictEqual(['space-x-9'])
  expect(await blocked('leading-4')).toStrictEqual(['leading-4'])
  expect(await blocked('translate-x-3')).toStrictEqual(['translate-x-3'])
  expect(await blocked('-translate-y-3')).toStrictEqual(['-translate-y-3'])
})

/**
 * The extractor reads every word in the interface's copy, and a bare margin or
 * padding shorthand resolves from the absent `--spacing` base — so the word
 * **me** in a sentence would otherwise put a dead rule in the stylesheet.
 */
it('blocks the spacing shorthands that ordinary words spell', async () => {
  expect(await blocked('me')).toStrictEqual(['me'])
  expect(await blocked('ms')).toStrictEqual(['ms'])
  expect(await blocked('px')).toStrictEqual(['px'])
  expect(await blocked('pt')).toStrictEqual(['pt'])

  const css = await generate('Show me the change first')

  expect(css).not.toContain('var(--spacing)')
})

it('emits the row divider through a :not(:last-of-type) selector', async () => {
  const css = await generate('row-divider')

  expect(css).toContain(
    '.row-divider:not(:last-of-type){border-bottom-width:1px;border-bottom-style:solid;border-bottom-color:#2e2e32;}',
  )
})

it('emits the group divider through an adjacent-sibling selector', async () => {
  const css = await generate('group-divider')

  expect(css).toContain(
    '.group-divider + .group-divider{border-top-width:1px;border-top-style:solid;border-top-color:#2e2e32;}',
  )
})

/**
 * A row's hover detail is revealed by the browser rather than by component
 * state, so that hiding the popover or reordering the list under a still
 * pointer cannot leave a resting row showing it. That only works if the classes
 * the row uses generate real CSS — the strict preset says nothing about a class
 * it does not know.
 *
 * The two named groups are what keeps the reveal from moving anything: the
 * outer one gives every detail its space the moment the pointer is on the list,
 * the inner one paints only the detail of the row under it. Both overrides beat
 * their resting utility on specificity, so neither depends on the order the
 * rules happen to be emitted in.
 */
it('reserves an element from the outer group and paints it from the inner one', async () => {
  const uno = await createGenerator(config)
  const { css } = await uno.generate(
    'group/list group/row hidden invisible group-hover/list:flex group-hover/row:visible',
  )

  expect(css).toContain('.hidden{display:none;}')
  expect(css).toContain(
    '.group\\/list:hover .group-hover\\/list\\:flex{display:flex;}',
  )
  expect(css).toContain('.invisible{visibility:hidden;}')
  expect(css).toContain(
    '.group\\/row:hover .group-hover\\/row\\:visible{visibility:visible;}',
  )
  expect([...uno.blocked]).toStrictEqual([])
})

it('emits the custom radius and popover width tokens', async () => {
  const css = await generate('rounded rounded-small rounded-large w-popover')

  expect(css).toContain('.rounded{border-radius:6px;}')
  expect(css).toContain('.rounded-small{border-radius:4px;}')
  expect(css).toContain('.rounded-large{border-radius:10px;}')
  expect(css).toContain('.w-popover{width:320px;}')
})

// The popover shell's frame: a 10px corner and a 1px neutral-4 hairline
it('emits the popover shell frame', async () => {
  const uno = await createGenerator(config)
  const { css } = await uno.generate(
    'h-screen w-popover overflow-hidden rounded-large border border-neutral-4 bg-neutral-1',
  )

  expect(css).toContain('.rounded-large{border-radius:10px;}')
  expect(css).toContain('.border{border-width:1px;}')
  expect(css).toContain('.h-screen{height:100vh;}')
  expect(css).toContain('--colors-neutral-4: #3a3a3f')
  expect(css).toContain('.border-neutral-4{border-color:')
  expect([...uno.blocked]).toStrictEqual([])
})

// Unread is carried by weight and brightness, never colour
it('resolves the unread and read title shortcuts', async () => {
  const css = await generate('title-unread title-read')

  expect(css).toContain('--colors-neutral-7: #ececef')
  expect(css).toContain('--fontWeight-3: 600')
  expect(css).toContain('--colors-neutral-6: #a0a0aa')
  expect(css).toContain('--fontWeight-1: 400')
  expect(css).toContain('.title-unread{color:')
  expect(css).toContain('.title-read{color:')
})

it('blocks arbitrary values and tokens outside the theme', async () => {
  expect(await blocked('p-[13px]')).toStrictEqual(['p-[13px]'])
  expect(await blocked('text-9')).toStrictEqual(['text-9'])
  expect(await blocked('rounded-sm')).toStrictEqual(['rounded-sm'])
  expect(await blocked('w-[320px]')).toStrictEqual(['w-[320px]'])
})
