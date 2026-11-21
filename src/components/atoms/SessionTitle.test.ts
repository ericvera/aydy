import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import SessionTitle from './SessionTitle.vue'

const title = (label: string, unread: boolean): ReturnType<typeof mount> =>
  mount(SessionTitle, { props: { label, unread } })

/**
 * Unread is carried by weight and brightness and never by colour, so that the
 * status dot stays the only thing in a row that means status. `title-unread`
 * and `title-read` are the shortcuts that encode the pair.
 */
it('sits an unread title forward at the heavy weight', () => {
  expect(
    title('Review attribution mock data', true).html(),
  ).toMatchInlineSnapshot(
    `"<span class="truncate text-2 title-unread">Review attribution mock data</span>"`,
  )
})

it('lets a read title recede to the light weight', () => {
  expect(
    title('Review attribution mock data', false).html(),
  ).toMatchInlineSnapshot(
    `"<span class="truncate text-2 title-read">Review attribution mock data</span>"`,
  )
})

it('never borrows a status colour to say unread', () => {
  const unread = title('Review attribution mock data', true).html()
  const read = title('Review attribution mock data', false).html()

  expect(unread).not.toMatch(/status-/)
  expect(read).not.toMatch(/status-/)
})

it('renders the title it is given', () => {
  expect(renderedText(title('Fix permission denied', false).element)).toBe(
    'Fix permission denied',
  )
})

/**
 * The whole fallback chain, placeholder included, runs in Rust, and the label
 * arrives resolved. A copy of the placeholder here would be a second set of
 * user-visible words saying the same thing: changing the Rust one would leave
 * this row rendering the old wording with both suites still green. So the atom
 * writes what it is given and nothing else, even when that is nothing.
 */
it('invents no wording of its own for a label that arrives empty', () => {
  expect(renderedText(title('', true).element)).toBe('')
})
