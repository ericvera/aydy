import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import Banner from './Banner.vue'

const banner = (variant: 'notice' | 'warning' | 'error'): string[] =>
  mount(Banner, { props: { variant }, slots: { default: 'Something' } })
    .classes()
    .sort()

it('shows what it is given', () => {
  const wrapper = mount(Banner, {
    props: { variant: 'notice' },
    slots: { default: 'Port 47823 was taken.' },
  })

  expect(wrapper.text()).toBe('Port 47823 was taken.')
})

/**
 * The status tints exist for exactly this. Two variants sharing a tint would
 * make a self-resolved notice and a failure look the same.
 */
it('gives each variant a tint of its own', () => {
  const tints = [banner('notice'), banner('warning'), banner('error')].map(
    (classes) => classes.filter((name) => name.startsWith('bg-')).join(),
  )

  expect(tints).toStrictEqual([
    'bg-neutral-3',
    'bg-statustransparent-needs-input',
    'bg-statustransparent-error',
  ])
})

it('is only ever one tint at a time', () => {
  for (const classes of [
    banner('notice'),
    banner('warning'),
    banner('error'),
  ]) {
    expect(classes.filter((name) => name.startsWith('bg-'))).toHaveLength(1)
  }
})
