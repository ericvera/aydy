import { normalizeText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import PopoverHeader from './PopoverHeader.vue'

const header = (
  sessionCount: number,
  unreadCount: number,
  stale = false,
): VueWrapper =>
  mount(PopoverHeader, { props: { sessionCount, unreadCount, stale } })

const heading = (
  sessionCount: number,
  unreadCount: number,
  stale = false,
): string =>
  normalizeText(header(sessionCount, unreadCount, stale).get('p').text())

it('says there are no sessions when the registry is empty', () => {
  expect(heading(0, 0)).toBe('No sessions')
})

it('counts the sessions and the unread ones', () => {
  expect(heading(6, 2)).toBe('6 sessions · 2 new')
})

it('leaves the unread count off when nothing is unread', () => {
  expect(heading(4, 0)).toBe('4 sessions')
})

it('writes a single session in the singular', () => {
  expect(heading(1, 0)).toBe('1 session')
})

it('counts a single unread session too', () => {
  expect(heading(1, 1)).toBe('1 session · 1 new')
})

/**
 * A stale list is not a count. The registry goes on reaping sessions while
 * events cannot reach it, so the live count drains to zero underneath a notice
 * saying what was last seen — No sessions above Last seen 3 sessions.
 */
it('names the state rather than a count while the list is stale', () => {
  expect(heading(0, 0, true)).toBe('Reconnecting')
  expect(heading(3, 2, true)).toBe('Reconnecting')
})

/**
 * The list is the only way into settings, and the header is the only part of
 * the list that is always there — an empty list and a stale one both keep it.
 */
it('asks for settings from the header', async () => {
  const wrapper = header(0, 0)

  await wrapper.get('[aria-label="Settings"]').trigger('click')

  expect(wrapper.emitted('settings')).toHaveLength(1)
})

it('keeps the way into settings while the list is stale', async () => {
  const wrapper = header(0, 0, true)

  await wrapper.get('[aria-label="Settings"]').trigger('click')

  expect(wrapper.emitted('settings')).toHaveLength(1)
})
