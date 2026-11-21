import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import ExtensionPrompt from './ExtensionPrompt.vue'

const prompt = (): VueWrapper => mount(ExtensionPrompt)

/**
 * The three things the user is owed, in one line: that the extension exists,
 * what it adds, and that nothing is reporting right now.
 */
it('says what the extension adds and that nothing is reporting yet', () => {
  expect(renderedText(prompt().element)).toBe(
    'Add the VS Code extension It takes you to the tab a session is in, and keeps AYDY quiet about the session you are watching. No VS Code window is reporting to AYDY yet. ✕',
  )
})

/**
 * A dismissible line, never a modal: everything the extension adds is an
 * improvement on something that already works without it.
 */
it('can be waved away', async () => {
  const wrapper = prompt()

  await wrapper.get('[aria-label="Dismiss"]').trigger('click')

  expect(wrapper.emitted('dismiss')).toHaveLength(1)
})

/**
 * Status colour is reserved for status. An offer is not a warning, and
 * borrowing the amber would put it beside the dots that mean something.
 */
it('is drawn as a quiet line rather than a banner', () => {
  const classes = prompt().classes()

  expect(classes).toContain('border-neutral-3')
  expect(classes.join(' ')).not.toContain('status')
})
