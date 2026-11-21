import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { HookFailure } from '../../types/HookFailure'
import type { HookStatus } from '../../types/HookStatus'

import HookSettings from './HookSettings.vue'

const SETTINGS_PATH = '~/.claude/settings.json'

const aStatus = (overrides: Partial<HookStatus> = {}): HookStatus => ({
  installed: true,
  hookCount: 5,
  settingsPath: SETTINGS_PATH,
  receivingEvents: true,
  ...overrides,
})

const hooks = (
  status: HookStatus = aStatus(),
  failure: HookFailure | null = null,
  busy = false,
): VueWrapper => mount(HookSettings, { props: { status, failure, busy } })

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
 * The two facts, never collapsed into one. Hooks registered against a port
 * nothing is listening on are installed and silent, and that is exactly the
 * state the port-move and stale notices exist to explain — an app that reported
 * only that its hooks exist would say nothing about it at all.
 */
it('states whether the hooks are installed and whether events are arriving, separately', () => {
  expect(renderedText(hooks(aStatus()).element)).toContain(
    'Installed — 5 hooks Receiving events',
  )
  expect(
    renderedText(hooks(aStatus({ receivingEvents: false })).element),
  ).toContain('Installed — 5 hooks Not receiving events')
  expect(
    renderedText(
      hooks(aStatus({ installed: false, hookCount: 0, receivingEvents: false }))
        .element,
    ),
  ).toContain('Not installed Not receiving events')
})

/**
 * Installed and silent is the one state here worth acting on, so it says what
 * is wrong and which action fixes it.
 */
it('explains the installed-but-silent state and points at the repair', () => {
  const wrapper = hooks(aStatus({ receivingEvents: false }))

  expect(renderedText(wrapper.element)).toContain(
    'The hooks are reporting to a port AYDY is not listening on, so it cannot see your sessions. Reinstall points them back at it.',
  )
})

/**
 * It is the one state on this screen worth acting on, and it is the same
 * condition the stale notice raises as a warning over the session list. Two
 * readings of one fact — a warning there, the dimmest line here — is what the
 * shared treatment prevents.
 */
it('raises the installed-but-silent state as a warning, as the list does', () => {
  const banners = hooks(aStatus({ receivingEvents: false })).findAllComponents({
    name: 'Banner',
  })

  expect(banners.map((banner) => banner.props('variant'))).toStrictEqual([
    'warning',
  ])
  expect(hooks().findAllComponents({ name: 'Banner' })).toStrictEqual([])
})

it('says nothing about a port when everything is arriving', () => {
  expect(renderedText(hooks().element)).not.toContain('port')
})

/**
 * The count is the installer's, counted from what is registered. A five written
 * into this line would go on being right until the day a sixth hook was added.
 */
it('states the hook count the installer reported', () => {
  expect(renderedText(hooks(aStatus({ hookCount: 6 })).element)).toContain(
    'Installed — 6 hooks',
  )
  expect(renderedText(hooks(aStatus({ hookCount: 1 })).element)).toContain(
    'Installed — 1 hook',
  )
})

it('offers both actions', async () => {
  const wrapper = hooks()

  await press(wrapper, 'Reinstall')
  await press(wrapper, 'Remove hooks')

  expect(wrapper.emitted('reinstall')).toHaveLength(1)
  expect(wrapper.emitted('remove')).toHaveLength(1)
})

/**
 * Removing puts back configuration the install deleted, and the user has to
 * know that before they press it — it is the reason the removal was acceptable
 * in the first place. It also has to be clear the app is not going anywhere.
 */
it('says the removal restores the sound hooks and leaves the app running', () => {
  expect(renderedText(hooks().element)).toContain(
    `Removing takes AYDY's hooks out of ${SETTINGS_PATH} and puts back the sound hooks it replaced. AYDY keeps running and asks to install them again.`,
  )
})

/**
 * Both actions write the user's settings file, and every failure path leaves it
 * exactly as it was — which is the first thing to say about one.
 */
it('reports a failed change and that the file was left alone', () => {
  const rendered = renderedText(
    hooks(aStatus(), { reason: 'The file changed while AYDY was writing it.' })
      .element,
  )

  expect(rendered).toContain(`Nothing in ${SETTINGS_PATH} was modified.`)
  expect(rendered).toContain('The file changed while AYDY was writing it.')
})

it('offers neither action while one is running', () => {
  const wrapper = hooks(aStatus(), null, true)

  expect(
    wrapper.findAll('button').map((button) => button.attributes('disabled')),
  ).toStrictEqual(['', ''])
})
