import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import JumpFailureNotice from './JumpFailureNotice.vue'

const notice = (failure: 'unanswered' | 'duplicated'): VueWrapper =>
  mount(JumpFailureNotice, { props: { failure } })

/**
 * A click that did nothing would otherwise look like an app that ignored it,
 * and a window nobody heard from is most likely one that was closed.
 */
it('says no window took the jump when none answered it', () => {
  expect(renderedText(notice('unanswered').element)).toContain(
    'No VS Code window took the jump. The window that had this session may be closed.',
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
 * A jump no window answered changed nothing; a tab that opened is known to be
 * wrong. The two are not the same weight and are not drawn as though they
 * were.
 */
it('weighs an unanswered jump below a duplicated session', () => {
  expect(
    notice('unanswered').findComponent({ name: 'Banner' }).props('variant'),
  ).toBe('warning')
  expect(
    notice('duplicated').findComponent({ name: 'Banner' }).props('variant'),
  ).toBe('error')
})

it('can be dismissed', async () => {
  const wrapper = notice('unanswered')

  await wrapper.get('[aria-label="Dismiss"]').trigger('click')

  expect(wrapper.emitted('dismiss')).toHaveLength(1)
})
