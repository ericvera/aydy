import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import SettingRow from './SettingRow.vue'

it('names the setting and carries its control', () => {
  const wrapper = mount(SettingRow, {
    props: { label: 'Launch at login' },
    slots: { control: '<button>on</button>' },
  })

  expect(renderedText(wrapper.element)).toBe('Launch at login on')
})

/**
 * What a setting does, when its label alone does not say, has to sit with the
 * control it is about, which is what this slot is for.
 */
it('carries a note under the row', () => {
  const wrapper = mount(SettingRow, {
    props: { label: 'VS Code extension' },
    slots: { note: '<p>Everything else works without it.</p>' },
  })

  expect(renderedText(wrapper.element)).toBe(
    'VS Code extension Everything else works without it.',
  )
})
