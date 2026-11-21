import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import Toggle from './Toggle.vue'

const toggle = (checked: boolean): VueWrapper =>
  mount(Toggle, { props: { checked, label: 'Launch at login' } })

it('asks for the other value when it is pressed', async () => {
  const off = toggle(false)
  await off.trigger('click')

  expect(off.emitted('change')).toStrictEqual([[true]])

  const on = toggle(true)
  await on.trigger('click')

  expect(on.emitted('change')).toStrictEqual([[false]])
})

/**
 * The value the switch draws is the one it was given, never one it kept for
 * itself: the store decides what a change settles on, and a switch that moved
 * itself would show a state the app is not in.
 */
it('draws the value it was given rather than one of its own', async () => {
  const wrapper = toggle(false)

  await wrapper.trigger('click')

  expect(wrapper.attributes('aria-checked')).toBe('false')

  await wrapper.setProps({ checked: true })

  expect(wrapper.attributes('aria-checked')).toBe('true')
})

it('is named by what it switches', () => {
  expect(toggle(true).attributes('aria-label')).toBe('Launch at login')
})

/**
 * Status colour is reserved for session status. A control borrowing one is how
 * a coloured element stops being a reliable read of state — which is why the
 * on state is the brand colour rather than the green the mock draws.
 */
it('carries no status colour', () => {
  const classes = [
    ...toggle(true).classes(),
    ...toggle(false).classes(),
    ...toggle(true).get('span').classes(),
  ]

  expect(classes.filter((name) => name.includes('status'))).toStrictEqual([])
  expect(toggle(true).classes()).toContain('bg-brand-primary')
})

it('shows on and off as different states', () => {
  expect(toggle(true).classes()).not.toStrictEqual(toggle(false).classes())
  expect(toggle(true).get('span').classes()).toContain('end-1')
  expect(toggle(false).get('span').classes()).toContain('start-1')
})
