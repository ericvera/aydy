import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { HookFailure } from '../../types/HookFailure'
import type { HookPreview } from '../../types/HookPreview'
import FirstRun from './FirstRun.vue'

const SETTINGS_PATH = '~/.claude/settings.json'

const aPreview = (overrides: Partial<HookPreview> = {}): HookPreview => ({
  hookCount: 5,
  added: [
    { event: 'SessionStart', kind: 'command', timeoutSeconds: 2 },
    { event: 'Stop', kind: 'http', timeoutSeconds: 2 },
  ],
  removed: [],
  hooksJson: '{\n  "Stop": []\n}',
  ...overrides,
})

const firstRun = (
  preview: HookPreview | null = aPreview(),
  failure: HookFailure | null = null,
  busy = false,
): VueWrapper =>
  mount(FirstRun, {
    props: { settingsPath: SETTINGS_PATH, preview, failure, busy },
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

it('explains what it will write, and where', () => {
  expect(renderedText(firstRun().element)).toContain(
    `AYDY adds 5 hooks to ${SETTINGS_PATH} so every project reports in. Everything else in that file is left untouched.`,
  )
})

/**
 * The number has to be the number the installer will actually register. A
 * literal here is what would keep saying five after a sixth hook was added.
 */
it('states the hook count the installer reported', () => {
  expect(renderedText(firstRun(aPreview({ hookCount: 6 })).element)).toContain(
    'AYDY adds 6 hooks to',
  )
  expect(renderedText(firstRun(aPreview({ hookCount: 1 })).element)).toContain(
    'AYDY adds 1 hook to',
  )
})

/**
 * Deleting entries the user wrote is the most consequential thing this app
 * does, so every one of them is named before the button that does it.
 */
it('names every entry it will remove, and says what replaces them', () => {
  const wrapper = firstRun(
    aPreview({
      removed: [
        {
          event: 'Stop',
          command: 'afplay /System/Library/Sounds/Funk.aiff',
        },
        {
          event: 'Notification',
          command: 'afplay /System/Library/Sounds/Hero.aiff',
        },
      ],
    }),
  )

  const text = renderedText(wrapper.element)

  expect(text).toContain('Stop — afplay /System/Library/Sounds/Funk.aiff')
  expect(text).toContain(
    'Notification — afplay /System/Library/Sounds/Hero.aiff',
  )
  expect(text).toContain(
    'Installing removes the 2 hooks below, which play a sound when a session finishes or needs input. AYDY plays those sounds itself, so you do not hear both, and you can turn its sounds off afterwards.',
  )
})

it('says nothing about removals when it will remove nothing', () => {
  expect(renderedText(firstRun().element)).not.toContain('Installing removes')
})

/**
 * Nothing is written until the user says so.
 */
it('installs nothing until it is asked to', () => {
  expect(firstRun().emitted('install')).toBeUndefined()
})

it('installs when the install is confirmed', async () => {
  const wrapper = firstRun()

  await press(wrapper, 'Install hooks')

  expect(wrapper.emitted('install')).toHaveLength(1)
})

/**
 * The change is offered before it is made, and what is shown is the plan the
 * installer will commit.
 */
it('shows the exact change when it is asked for, and not before', async () => {
  const wrapper = firstRun()

  expect(wrapper.findComponent({ name: 'HookChangePreview' }).exists()).toBe(
    false,
  )

  await press(wrapper, 'Show me the change first')

  expect(
    wrapper.findComponent({ name: 'HookChangePreview' }).props('preview'),
  ).toStrictEqual(aPreview())
  expect(wrapper.emitted('install')).toBeUndefined()
})

/**
 * The emphasis is the point: the user has just been told an edit to their own
 * settings file failed, and needs to know the file is intact.
 */
it('reports that nothing was modified when the write failed', () => {
  const wrapper = firstRun(aPreview(), {
    reason: 'The file changed while AYDY was writing it.',
  })

  expect(renderedText(wrapper.element)).toContain(
    `Nothing in ${SETTINGS_PATH} was modified. The file changed while AYDY was writing it.`,
  )
})

it('offers both a retry and a way to look at the file after a failure', async () => {
  const wrapper = firstRun(aPreview(), { reason: 'It did not work.' })

  await press(wrapper, 'Try again')
  await press(wrapper, 'Reveal file')

  expect(wrapper.emitted('install')).toHaveLength(1)
  expect(wrapper.emitted('reveal')).toHaveLength(1)
})

it('offers no retry until something has failed', () => {
  const labels = firstRun()
    .findAll('button')
    .map((button) => button.text())

  expect(labels).toStrictEqual(['Install hooks', 'Show me the change first'])
})

/**
 * The settings file could not be read or planned against, so there is no
 * honest count to state and nothing to install from — but the failure and the
 * way to go and look at the file still are.
 */
it('states no count when the installer could not plan the change', () => {
  const text = renderedText(
    firstRun(null, { reason: 'The file is not valid JSON.' }).element,
  )

  expect(text).toContain(`AYDY adds its hooks to ${SETTINGS_PATH}`)
  expect(text).not.toContain('AYDY adds 0 hooks')
})

/**
 * Nothing on this screen may write to the settings file while the change has
 * not been planned: the plan is what names the entries the install deletes, and
 * the removal warning is rendered from it. Retrying a failed plan plans again.
 */
it('offers no action that installs when the change could not be planned', async () => {
  const wrapper = firstRun(null, { reason: 'AYDY is not listening yet.' })

  const buttons = wrapper.findAll('button')

  for (const button of buttons) {
    await button.trigger('click')
  }

  expect(buttons.map((button) => button.text())).toStrictEqual([
    'Try again',
    'Reveal file',
  ])
  expect(wrapper.emitted('install')).toBeUndefined()
  expect(wrapper.emitted('plan')).toHaveLength(1)
  expect(wrapper.emitted('reveal')).toHaveLength(1)
})

/**
 * Once the change has been planned and the removals named, the write is what
 * failed and the write is what is retried.
 */
it('retries the install itself when the write is what failed', async () => {
  const wrapper = firstRun(aPreview(), { reason: 'It did not work.' })

  await press(wrapper, 'Try again')

  expect(wrapper.emitted('install')).toHaveLength(1)
  expect(wrapper.emitted('plan')).toBeUndefined()
})

it('says it is installing while the install is running', () => {
  const labels = firstRun(aPreview(), null, true)
    .findAll('button')
    .map((button) => button.text())

  expect(labels).toContain('Installing…')
})
