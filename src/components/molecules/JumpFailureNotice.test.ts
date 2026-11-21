import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import JumpFailureNotice from './JumpFailureNotice.vue'

const notice = (failure: 'unconfirmed' | 'duplicated'): VueWrapper =>
  mount(JumpFailureNotice, { props: { failure } })

/**
 * Every mechanism a jump is made of fails silently, so a click that did
 * nothing would otherwise look like an app that ignored it.
 */
it('says the window never came forward when nothing confirmed the jump', () => {
  expect(renderedText(notice('unconfirmed').element)).toContain(
    'VS Code did not come forward. AYDY asked twice and heard nothing back.',
  )
})

/**
 * The tab may be a second live copy of the session, so the first thing said is
 * the thing that keeps it from becoming one the user has typed into.
 */
it('says what to do about a tab that opened instead', () => {
  expect(renderedText(notice('duplicated').element)).toContain(
    'A new Claude tab opened. Close it without typing in it — it may be a second copy of that session.',
  )
})

/**
 * A jump nothing confirmed may well have worked; a tab that opened is known to
 * be wrong. The two are not the same weight and are not drawn as though they
 * were.
 */
it('weighs an unconfirmed jump below a duplicated session', () => {
  expect(
    notice('unconfirmed').findComponent({ name: 'Banner' }).props('variant'),
  ).toBe('warning')
  expect(
    notice('duplicated').findComponent({ name: 'Banner' }).props('variant'),
  ).toBe('error')
})

it('can be dismissed', async () => {
  const wrapper = notice('unconfirmed')

  await wrapper.get('[aria-label="Dismiss"]').trigger('click')

  expect(wrapper.emitted('dismiss')).toHaveLength(1)
})
