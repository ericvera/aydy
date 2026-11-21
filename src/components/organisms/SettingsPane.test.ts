import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { Settings } from '../../types/Settings'

import SettingsPane from './SettingsPane.vue'

const aSettings = (overrides: Partial<Settings> = {}): Settings => ({
  finishedSound: true,
  needsInputSound: true,
  launchAtLogin: true,
  ...overrides,
})

interface Options {
  settings?: Settings | null
  failure?: { reason: string } | null
}

const pane = (options: Options = {}): VueWrapper =>
  mount(SettingsPane, {
    props: {
      settings: options.settings === undefined ? aSettings() : options.settings,
      failure: options.failure ?? null,
    },
  })

const switches = (wrapper: VueWrapper): string[] =>
  wrapper
    .findAll('[role="switch"]')
    .map((toggle) => toggle.attributes('aria-label') ?? '')

/**
 * Everything REQ-SET-2 requires the pane to expose, in one place, so a control
 * that goes missing is a failure here rather than something noticed in use.
 */
it('exposes every setting the pane is responsible for', () => {
  const wrapper = pane()
  const rendered = renderedText(wrapper.element)

  expect(switches(wrapper)).toStrictEqual([
    'When a session finishes',
    'When a session needs input',
    'Launch at login',
  ])
  expect(rendered).toContain('VS Code extension')
  expect(rendered).toContain('Session tracking Installed')
})

/**
 * The port appears nowhere in the interface: choosing one is the decision the
 * app exists to make for itself, and there is no field on this screen at all.
 * Mock scenario 5's caption still describes a port field; it is stale.
 */
it('has no listener port field', () => {
  const wrapper = pane()

  expect(wrapper.findAll('input')).toStrictEqual([])
  expect(renderedText(wrapper.element).toLowerCase()).not.toContain('port')
})

it('goes back to the list', async () => {
  const wrapper = pane()

  await wrapper.get('[aria-label="Close settings"]').trigger('click')

  expect(wrapper.emitted('close')).toHaveLength(1)
})

/**
 * A control sends only itself. Sending back the values it did not touch is how
 * a stale copy of the settings overwrites a change made somewhere else.
 */
it('sends only the setting that moved', async () => {
  const wrapper = pane()

  await wrapper.get('[aria-label="When a session finishes"]').trigger('click')
  await wrapper.get('[aria-label="Launch at login"]').trigger('click')

  expect(wrapper.emitted('update')).toStrictEqual([
    [{ finishedSound: false }],
    [{ launchAtLogin: false }],
  ])
})

/**
 * The pane is reachable only once tracking is installed, so the row has one
 * state to show, and the note says who records the sessions.
 */
it('reports session tracking as installed by the extension', () => {
  const rendered = renderedText(pane().element)

  expect(rendered).toContain('Session tracking Installed')
  expect(rendered).toContain(
    'The extension records every Claude Code session, whether or not AYDY is running. AYDY only reads what it records.',
  )
})

it('has no hooks section', () => {
  const rendered = renderedText(pane().element).toLowerCase()

  expect(rendered).not.toContain('hook')
})

/**
 * A settings command that failed has to say so: the write is the only failure
 * where the change did happen, and a pane that swallowed the reason would leave
 * the user setting it twice.
 */
it('reports why a settings command failed', () => {
  const rendered = renderedText(
    pane({
      failure: {
        reason:
          'The change is in force, but AYDY could not save it for next time.',
      },
    }).element,
  )

  expect(rendered).toContain(
    'The change is in force, but AYDY could not save it for next time.',
  )
})

/**
 * The way back has to survive a settings read that has not landed, or failed:
 * a pane with nothing in it and no way out is the worse of the two failures.
 */
it('keeps the way back while there are no settings to draw', () => {
  const wrapper = pane({
    settings: null,
    failure: { reason: 'AYDY could not finish reading your settings.' },
  })

  expect(wrapper.find('[aria-label="Close settings"]').exists()).toBe(true)
  expect(renderedText(wrapper.element)).toContain(
    'AYDY could not finish reading your settings.',
  )
  expect(switches(wrapper)).toStrictEqual([])
})
