import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import StaleListNotice from './StaleListNotice.vue'

const notice = (sessionCount: number, lastSeenAgoMs: number): string =>
  renderedText(
    mount(StaleListNotice, { props: { sessionCount, lastSeenAgoMs } }).element,
  )

/**
 * A list that is not being updated must not be presented as current, and an
 * empty one would read as "nothing is running".
 */
it('says the list is stale', () => {
  expect(notice(3, 360_000)).toContain(
    'AYDY is not receiving session events, so this list is stale.',
  )
})

it('states how many sessions were last seen, and when', () => {
  expect(notice(3, 360_000)).toContain('Last seen 3 sessions, 6m ago.')
  expect(notice(1, 45_000)).toContain('Last seen 1 session, 45s ago.')
  expect(notice(0, 7_200_000)).toContain('Last seen no sessions, 2h 0m ago.')
})

/**
 * The list is stale, which is a fault of the app's own — not the amber of a
 * session waiting on the user, and not a failure the user can retry.
 */
it('is a warning rather than an error', () => {
  const wrapper = mount(StaleListNotice, {
    props: { sessionCount: 3, lastSeenAgoMs: 360_000 },
  })

  expect(wrapper.findComponent({ name: 'Banner' }).props('variant')).toBe(
    'warning',
  )
})
