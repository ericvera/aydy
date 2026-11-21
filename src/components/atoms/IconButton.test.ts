import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import IconButton from './IconButton.vue'

const button = (label = 'Settings'): ReturnType<typeof mount> =>
  mount(IconButton, { props: { label }, slots: { default: '⚙' } })

it('presses', async () => {
  const wrapper = button()

  await wrapper.get('button').trigger('click')

  expect(wrapper.emitted('press')).toHaveLength(1)
})

/**
 * The glyph carries no meaning to anything that cannot see it, so the label is
 * the only name this control has — and it is what every test reaches it by.
 */
it('names itself for anything that cannot see the glyph', () => {
  const wrapper = button('Close settings')

  expect(wrapper.get('button').attributes('aria-label')).toBe('Close settings')
  expect(wrapper.text()).toBe('⚙')
})

/**
 * A bare glyph, not a chip: `ActionButton` is the control with a box around it,
 * and a header gear drawn like one would outweigh the screen it heads.
 */
it('draws as a bare glyph', () => {
  expect(button().classes().sort()).toMatchInlineSnapshot(`
    [
      "px-2",
      "py-1",
      "rounded-small",
      "text-2",
      "text-neutral-6",
    ]
  `)
})
