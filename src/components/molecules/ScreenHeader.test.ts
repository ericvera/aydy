import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import ScreenHeader from './ScreenHeader.vue'

it('heads the screen with its title', () => {
  const wrapper = mount(ScreenHeader, { props: { title: 'Settings' } })

  expect(wrapper.get('header p').text()).toBe('Settings')
})

/**
 * The control on the right is passed in, and this is the whole reason the
 * header is shared rather than each screen keeping its own: the gear belongs to
 * the list alone. The screens that ask for the hooks use this same header and
 * pass nothing, so anything built into it would appear on them too — where
 * installing the hooks is the only thing that can be done.
 */
it('carries no control of its own', () => {
  const wrapper = mount(ScreenHeader, { props: { title: 'Setup' } })

  expect(wrapper.findAll('button')).toHaveLength(0)
})

it('carries the control the screen gave it', () => {
  const wrapper = mount(ScreenHeader, {
    props: { title: 'No sessions' },
    slots: { default: '<button type="button">⚙</button>' },
  })

  expect(wrapper.findAll('button')).toHaveLength(1)
})

/**
 * The popover is a column between a fixed header and a fixed footer, so the
 * header must not be given away to a list long enough to scroll.
 */
it('does not shrink', () => {
  const wrapper = mount(ScreenHeader, { props: { title: 'Settings' } })

  expect(wrapper.get('header').classes()).toContain('shrink-0')
})
