import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import EmptyState from './EmptyState.vue'

/**
 * The empty state explains how a session gets here, so a user looking at an
 * empty popover on a healthy app is not left wondering whether it is broken.
 */
it('explains that starting Claude Code is all it takes', () => {
  expect(renderedText(mount(EmptyState).element)).toBe(
    'Nothing running Start Claude Code in any project and it shows up here automatically.',
  )
})

/**
 * The approved mock draws a daily working-time total beneath this, which does
 * not ship: aggregate durations across sessions are out of scope, and the
 * total would need sessions to be retained past their end.
 */
it('carries no aggregate total', () => {
  expect(mount(EmptyState).text()).not.toMatch(/total|Total|\d+h|\d+m/)
})
