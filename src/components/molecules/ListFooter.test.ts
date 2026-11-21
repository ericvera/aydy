import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import ListFooter from './ListFooter.vue'

it('offers quit and nothing else', () => {
  expect(renderedText(mount(ListFooter).element)).toBe('Quit')
})

it('asks to quit when quit is pressed', async () => {
  const wrapper = mount(ListFooter)

  await wrapper.get('button').trigger('click')

  expect(wrapper.emitted('quit')).toStrictEqual([[]])
})

it('draws the footer', () => {
  expect(mount(ListFooter).html()).toMatchInlineSnapshot(
    `"<footer class="shrink-0 border-t border-neutral-3 px-5 py-3"><button type="button" class="text-1 text-neutral-6"> Quit </button></footer>"`,
  )
})
