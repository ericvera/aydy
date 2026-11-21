import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import SettingsUnreadable from './SettingsUnreadable.vue'

const unreadable = (busy = false): VueWrapper =>
  mount(SettingsUnreadable, {
    props: { failure: { reason: 'The file could not be read.' }, busy },
  })

const press = async (wrapper: VueWrapper, label: string): Promise<void> => {
  const button = wrapper
    .findAll('button')
    .find((candidate) => candidate.text() === label)

  if (button === undefined) {
    throw new Error(`there is no button labelled ${label}`)
  }

  await button.trigger('click')
}

/**
 * The read is what failed, so nothing was written — and the user is being told
 * about their own settings file.
 */
it('says why the file could not be read, and that nothing was modified', () => {
  const text = renderedText(unreadable().element)

  expect(text).toContain('AYDY cannot read your settings')
  expect(text).toContain('Nothing was modified. The file could not be read.')
})

it('offers a retry and a way to look at the file', async () => {
  const wrapper = unreadable()

  await press(wrapper, 'Try again')
  await press(wrapper, 'Reveal file')

  expect(wrapper.emitted('retry')).toHaveLength(1)
  expect(wrapper.emitted('reveal')).toHaveLength(1)
})

/**
 * Nothing here writes: the app cannot even tell whether its hooks are there,
 * and the entries an install would remove have never been named.
 */
it('offers nothing that installs', () => {
  const labels = unreadable()
    .findAll('button')
    .map((button) => button.text())

  expect(labels).toStrictEqual(['Try again', 'Reveal file'])
})
