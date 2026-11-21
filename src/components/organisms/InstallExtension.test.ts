import { normalizeText, renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import InstallExtension from './InstallExtension.vue'

const screen = (state: 'notInstalled' | 'hooksMissing'): VueWrapper =>
  mount(InstallExtension, { props: { state } })

it('asks for the extension when tracking was never installed', () => {
  expect(renderedText(screen('notInstalled').element)).toBe(
    'Setup Install the AYDY extension in VS Code The extension tracks your Claude Code sessions, even while this app is closed. AYDY shows what it records. Show me how Already installed? Open any VS Code window and this screen goes away.',
  )
})

it('shows the install steps inline once they are asked for', async () => {
  const wrapper = screen('notInstalled')

  await wrapper.get('button').trigger('click')

  const text = normalizeText(wrapper.element.textContent ?? '')

  expect(text).toContain(
    'In VS Code, open the Command Palette. Run Extensions: Install from VSIX… Choose aydy-companion.vsix. Open any project window.',
  )
  expect(text).toContain('Install the AYDY extension in VS Code')
  expect(wrapper.find('button').exists()).toBe(false)
})

it('says the hooks are missing when the extension is there without them', () => {
  const wrapper = screen('hooksMissing')

  expect(renderedText(wrapper.element)).toContain(
    "Setup AYDY's hooks are missing",
  )
  expect(normalizeText(wrapper.element.textContent ?? '')).toContain(
    'The extension is installed, but its hooks are not in ~/.claude/settings.json, so sessions are not being recorded. Open any VS Code window and the extension puts them back.',
  )
  expect(wrapper.find('button').exists()).toBe(false)
})

/** Nothing can be done from here but install, so there is no gear. */
it('offers no way into settings', () => {
  expect(screen('notInstalled').find('[aria-label="Settings"]').exists()).toBe(
    false,
  )
})
