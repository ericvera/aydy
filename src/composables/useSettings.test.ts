import { expect, it, vi } from 'vitest'

import type { Settings } from '../types/Settings'

interface Invocation {
  command: string
  args: Record<string, unknown> | undefined
}

const DEFAULTS: Settings = {
  finishedSound: true,
  needsInputSound: true,
  launchAtLogin: true,
}

interface Answers {
  settings: Settings | null
  fails: Record<string, unknown>
}

// Hoisted above the module body, so the defaults above are not in scope yet;
// every test sets this through `load`.
const { invoked, answers } = vi.hoisted(
  (): { invoked: Invocation[]; answers: Answers } => ({
    invoked: [],
    answers: { settings: null, fails: {} },
  }),
)

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (
    command: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> => {
    invoked.push({ command, args })

    if (command in answers.fails) {
      throw answers.fails[command]
    }

    return answers.settings
  },
}))

/**
 * A fresh copy of the composable, since its state is module scoped on purpose.
 */
const load = async (
  settings: Settings = DEFAULTS,
  fails: Record<string, unknown> = {},
): Promise<typeof import('./useSettings')> => {
  invoked.length = 0
  answers.settings = settings
  answers.fails = fails
  vi.resetModules()

  return import('./useSettings')
}

it('does nothing until it is asked to', async () => {
  const { useSettings } = await load()

  useSettings()

  expect(invoked).toStrictEqual([])
})

it('reads the settings when the pane opens', async () => {
  const { useSettings } = await load()
  const pane = useSettings()

  await pane.read()

  expect(pane.settings.value).toStrictEqual(DEFAULTS)
  expect(pane.failure.value).toBeNull()
  expect(invoked).toStrictEqual([{ command: 'read_settings', args: undefined }])
})

/**
 * A control sends only itself, so a pane rendered from a copy taken a moment
 * ago cannot put back a value something else has changed since.
 */
it('sends only the setting that moved', async () => {
  const { useSettings } = await load()

  await useSettings().update({ needsInputSound: false })

  expect(invoked).toStrictEqual([
    {
      command: 'update_settings',
      args: { change: { needsInputSound: false } },
    },
  ])
})

/**
 * What comes back is what the app is in — not what the pane asked for.
 * Rendering the request instead would show a state that does not exist
 * anywhere.
 */
it('takes the answer the store settled on rather than what it asked for', async () => {
  const settled: Settings = { ...DEFAULTS, launchAtLogin: true }
  const { useSettings } = await load(settled)
  const pane = useSettings()

  await pane.update({ launchAtLogin: false })

  expect(pane.settings.value).toStrictEqual(settled)
})

/**
 * A settings file that could not be written is the one failure where the change
 * did happen: the store publishes before it persists. Leaving the pane showing
 * the value it started from would contradict the reason it is being given, so
 * the settings are read back and only the reason is kept.
 */
it('reads back what is actually in force when a change could not be saved', async () => {
  const inForce: Settings = { ...DEFAULTS, finishedSound: false }
  const { useSettings } = await load(inForce, {
    update_settings: {
      reason:
        'The change is in force, but AYDY could not save it for next time.',
    },
  })
  const pane = useSettings()

  await pane.update({ finishedSound: false })

  expect(pane.failure.value).toStrictEqual({
    reason: 'The change is in force, but AYDY could not save it for next time.',
  })
  expect(pane.settings.value).toStrictEqual(inForce)
  expect(invoked.map((call) => call.command)).toStrictEqual([
    'update_settings',
    'read_settings',
  ])
  expect(pane.busy.value).toBe(false)
})

/**
 * The reason the user needs is the one about the change they made, not a second
 * one about the read that followed it.
 */
it('keeps the reason the change failed even when the read after it fails too', async () => {
  const { useSettings } = await load(DEFAULTS, {
    update_settings: {
      reason: 'AYDY could not change whether it starts when you log in.',
    },
    read_settings: { reason: 'AYDY could not finish reading your settings.' },
  })
  const pane = useSettings()

  await pane.update({ launchAtLogin: false })

  expect(pane.failure.value).toStrictEqual({
    reason: 'AYDY could not change whether it starts when you log in.',
  })
})

it('clears the last failure once a change lands', async () => {
  const { useSettings } = await load(DEFAULTS, {
    update_settings: {
      reason: 'AYDY could not change whether it starts when you log in.',
    },
  })
  const pane = useSettings()

  await pane.update({ launchAtLogin: false })
  answers.fails = {}
  await pane.update({ launchAtLogin: false })

  expect(pane.failure.value).toBeNull()
})

it('reports a read it could not make', async () => {
  const { useSettings } = await load(DEFAULTS, {
    read_settings: { reason: 'AYDY could not finish reading your settings.' },
  })
  const pane = useSettings()

  await pane.read()

  expect(pane.settings.value).toBeNull()
  expect(pane.failure.value).toStrictEqual({
    reason: 'AYDY could not finish reading your settings.',
  })
})

/**
 * Anything that rejects with something else is a bug rather than a state the
 * user can act on, and must not be rendered as one.
 */
it('does not present an unexplained rejection as a reason', async () => {
  const { useSettings } = await load(DEFAULTS, {
    read_settings: new Error('boom'),
  })
  const pane = useSettings()

  await pane.read()

  expect(pane.failure.value).toStrictEqual({
    reason: 'AYDY could not tell what went wrong.',
  })
})
