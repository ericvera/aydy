import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import ActionButton from './ActionButton.vue'

it('presses', async () => {
  const wrapper = mount(ActionButton, {
    props: { variant: 'primary' },
    slots: { default: 'Install hooks' },
  })

  await wrapper.trigger('click')

  expect(wrapper.emitted('press')).toHaveLength(1)
})

/**
 * The action the screen is about is the brand colour; anything beside it is a
 * neutral chip. A screen inventing its own pair is how two buttons start
 * competing for the same emphasis.
 */
it('draws the primary action in the brand colour and the secondary one in neutral', () => {
  const classes = (variant: 'primary' | 'secondary'): string[] =>
    mount(ActionButton, { props: { variant } }).classes()

  expect(classes('primary')).toContain('bg-brand-primary')
  expect(classes('secondary')).toContain('bg-neutral-3')
  expect(classes('secondary')).not.toContain('bg-brand-primary')
})

/**
 * The primary action is disabled for the seconds an install takes, and a
 * brand-coloured button that does nothing reads as a click that was missed.
 */
it('looks disabled while it is disabled', () => {
  const disabled = mount(ActionButton, {
    props: { variant: 'primary', disabled: true },
  }).classes()

  expect(disabled).not.toContain('bg-brand-primary')
  expect(disabled).toContain('text-neutral-5')
})

/**
 * A button beside a field or under a settings row is smaller than one a whole
 * screen is about, and it keeps its variant's colours either way.
 */
it('draws the small treatment without changing what the variant means', () => {
  const small = mount(ActionButton, {
    props: { variant: 'secondary', size: 'small' },
  }).classes()
  const regular = mount(ActionButton, {
    props: { variant: 'secondary' },
  }).classes()

  expect(small).toContain('text-1')
  expect(small).toContain('bg-neutral-3')
  expect(small).not.toContain('text-2')
  expect(regular).toContain('text-2')
  expect(regular).not.toContain('text-1')
})

it('cannot be pressed while it is disabled', async () => {
  const wrapper = mount(ActionButton, {
    props: { variant: 'primary', disabled: true },
  })

  expect(wrapper.attributes('disabled')).toBeDefined()

  await wrapper.trigger('click')

  expect(wrapper.emitted('press')).toBeUndefined()
})
