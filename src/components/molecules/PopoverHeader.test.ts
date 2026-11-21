import { normalizeText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import PopoverHeader from './PopoverHeader.vue'

const header = (sessionCount: number, unreadCount: number): VueWrapper =>
  mount(PopoverHeader, { props: { sessionCount, unreadCount } })

const heading = (sessionCount: number, unreadCount: number): string =>
  normalizeText(header(sessionCount, unreadCount).get('p').text())

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
 * The list is the only way into settings, and the header is the only part of
 * the list that is always there — an empty list keeps it too.
 */
it('asks for settings from the header', async () => {
  const wrapper = header(0, 0)

  await wrapper.get('[aria-label="Settings"]').trigger('click')

  expect(wrapper.emitted('settings')).toHaveLength(1)
})
