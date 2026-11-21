import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { HookPreview } from '../../types/HookPreview'
import HookChangePreview from './HookChangePreview.vue'

const preview = (overrides: Partial<HookPreview> = {}): HookPreview => ({
  hookCount: 2,
  added: [
    { event: 'SessionStart', kind: 'command', timeoutSeconds: 2 },
    { event: 'SessionEnd', kind: 'http', timeoutSeconds: 0.5 },
  ],
  removed: [
    { event: 'Stop', command: 'afplay /System/Library/Sounds/Funk.aiff' },
  ],
  hooksJson: '{\n  "Stop": []\n}',
  ...overrides,
})

const shown = (overrides: Partial<HookPreview> = {}): string =>
  renderedText(
    mount(HookChangePreview, { props: { preview: preview(overrides) } })
      .element,
  )

/**
 * Both halves of the change, because "what will you add" and "what will you
 * take away" are different questions and only one of them is reversible by
 * doing nothing.
 */
it('shows every hook it will add, with how it runs and for how long', () => {
  const text = shown()

  expect(text).toContain('SessionStart · command · 2s timeout')
  expect(text).toContain('SessionEnd · http · 0.5s timeout')
})

it('shows every entry it will remove', () => {
  expect(shown()).toContain('Stop — afplay /System/Library/Sounds/Funk.aiff')
})

it('says nothing about removals when there are none', () => {
  expect(shown({ removed: [] })).not.toContain('Entries to be removed')
})

/**
 * The exact change means the exact text: this is the settings file's `hooks`
 * key as the installer will write it, not a description of it.
 */
it('shows the block as it will be written', () => {
  expect(shown()).toContain('{ "Stop": [] }')
})
