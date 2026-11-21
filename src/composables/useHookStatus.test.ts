import { expect, it, vi } from 'vitest'

import type { HookPreview } from '../types/HookPreview'
import type { HookStatus } from '../types/HookStatus'

interface Invocation {
  command: string
  args: Record<string, unknown> | undefined
}

const INSTALLED: HookStatus = {
  installed: true,
  hookCount: 5,
  settingsPath: '~/.claude/settings.json',
  receivingEvents: true,
}

const ABSENT: HookStatus = {
  installed: false,
  hookCount: 0,
  settingsPath: '~/.claude/settings.json',
  receivingEvents: false,
}

const PREVIEW: HookPreview = {
  hookCount: 5,
  added: [{ event: 'Stop', kind: 'http', timeoutSeconds: 2 }],
  removed: [{ event: 'Stop', command: 'afplay Funk.aiff' }],
  hooksJson: '{}',
}

interface Answers {
  hooks: HookStatus
  fails: Record<string, unknown>
}

const { invoked, answers } = vi.hoisted(
  (): { invoked: Invocation[]; answers: Answers } => ({
    invoked: [],
    answers: {
      hooks: {
        installed: false,
        hookCount: 0,
        settingsPath: '',
        receivingEvents: false,
      },
      fails: {},
    },
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

    if (command === 'hook_status') {
      return answers.hooks
    }

    return command === 'preview_hook_install' ? PREVIEW : undefined
  },
}))

/**
 * A fresh copy of the composable, since its state is module scoped on purpose.
 */
const load = async (
  hooks: HookStatus = ABSENT,
  fails: Record<string, unknown> = {},
): Promise<typeof import('./useHookStatus')> => {
  invoked.length = 0
  answers.hooks = hooks
  answers.fails = fails
  vi.resetModules()

  return import('./useHookStatus')
}

it('reads the hook status and the change it would make', async () => {
  const { useHookStatus } = await load()
  const hooks = useHookStatus()

  await hooks.refresh()

  expect(hooks.status.value).toStrictEqual(ABSENT)
  expect(hooks.preview.value).toStrictEqual(PREVIEW)
  expect(invoked.map((call) => call.command)).toStrictEqual([
    'hook_status',
    'preview_hook_install',
  ])
})

/**
 * Planning an install is a file read, and there is nothing to show it in once
 * the hooks are there.
 */
it('does not plan an install once the hooks are registered', async () => {
  const { useHookStatus } = await load(INSTALLED)
  const hooks = useHookStatus()

  await hooks.refresh()

  expect(hooks.preview.value).toBeNull()
  expect(invoked.map((call) => call.command)).toStrictEqual(['hook_status'])
})

it('installs and then reads the file back', async () => {
  const { useHookStatus } = await load()
  const hooks = useHookStatus()
  answers.hooks = INSTALLED

  await hooks.install()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'install_hooks',
    'hook_status',
  ])
  expect(hooks.status.value).toStrictEqual(INSTALLED)
  expect(hooks.failure.value).toBeNull()
  expect(hooks.busy.value).toBe(false)
})

/**
 * A failed install leaves the first-run screen up because the hooks really are
 * still absent — the status is read back from the file rather than assumed
 * either way.
 */
it('keeps the reason an install failed, and the screen that offers a retry', async () => {
  const { useHookStatus } = await load(ABSENT, {
    install_hooks: { reason: 'The file changed while AYDY was writing it.' },
  })
  const hooks = useHookStatus()

  await hooks.install()

  expect(hooks.failure.value).toStrictEqual({
    reason: 'The file changed while AYDY was writing it.',
  })
  expect(hooks.status.value).toStrictEqual(ABSENT)
  expect(hooks.busy.value).toBe(false)
})

it('clears the last failure when the install is tried again', async () => {
  const { useHookStatus } = await load(ABSENT, {
    install_hooks: { reason: 'The file changed while AYDY was writing it.' },
  })
  const hooks = useHookStatus()

  await hooks.install()
  answers.fails = {}
  answers.hooks = INSTALLED
  await hooks.install()

  expect(hooks.failure.value).toBeNull()
})

/**
 * The plan is what names the entries an install would delete, so a plan that
 * failed is retried by planning again — not by writing from a plan that does
 * not exist.
 */
it('plans the change again after a failed read, and clears the failure it replaces', async () => {
  const { useHookStatus } = await load(ABSENT, {
    preview_hook_install: { reason: 'AYDY is not listening yet.' },
  })
  const hooks = useHookStatus()

  await hooks.refresh()
  answers.fails = {}
  await hooks.plan()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'hook_status',
    'preview_hook_install',
    'hook_status',
    'preview_hook_install',
  ])
  expect(hooks.failure.value).toBeNull()
  expect(hooks.preview.value).toStrictEqual(PREVIEW)
  expect(hooks.busy.value).toBe(false)
})

it('keeps the reason a second read failed too', async () => {
  const { useHookStatus } = await load(ABSENT, {
    hook_status: { reason: 'The file could not be read.' },
  })
  const hooks = useHookStatus()

  await hooks.plan()

  expect(hooks.status.value).toBeNull()
  expect(hooks.failure.value).toStrictEqual({
    reason: 'The file could not be read.',
  })
})

it('reports a settings file it could not even read', async () => {
  const { useHookStatus } = await load(ABSENT, {
    preview_hook_install: { reason: 'The file is not valid JSON.' },
  })
  const hooks = useHookStatus()

  await hooks.refresh()

  expect(hooks.failure.value).toStrictEqual({
    reason: 'The file is not valid JSON.',
  })
  expect(hooks.preview.value).toBeNull()
})

/**
 * Anything that rejects with something else is a bug rather than a state the
 * user can act on, and must not be rendered as one.
 */
it('does not present an unexplained rejection as a reason', async () => {
  const { useHookStatus } = await load(ABSENT, {
    install_hooks: new Error('boom'),
  })
  const hooks = useHookStatus()

  await hooks.install()

  expect(hooks.failure.value).toStrictEqual({
    reason: 'AYDY could not tell what went wrong.',
  })
})

/**
 * A stubbed emit would pass a component test and ship a dead button, so the
 * reveal is a command like every other action.
 */
it('reveals the settings file through Rust', async () => {
  const { useHookStatus } = await load()

  await useHookStatus().revealSettingsFile()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'reveal_settings_file',
  ])
})

it('does nothing until it is asked to', async () => {
  const { useHookStatus } = await load()

  useHookStatus()

  expect(invoked).toStrictEqual([])
})

/**
 * Repairing the entries is its own command, not a second install: the pane
 * offers it for hooks that exist but are reporting somewhere the app is not
 * listening.
 */
it('repairs the hooks and reads the file back', async () => {
  const { useHookStatus } = await load(INSTALLED)
  const hooks = useHookStatus()

  await hooks.reinstall()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'reinstall_hooks',
    'hook_status',
  ])
  expect(hooks.status.value).toStrictEqual(INSTALLED)
  expect(hooks.failure.value).toBeNull()
})

/**
 * Removing them puts the first-run screen back, because the status is read from
 * the file afterwards and the hooks really are gone. Nothing about it ends the
 * app.
 */
it('removes the hooks and reads the file back', async () => {
  const { useHookStatus } = await load(INSTALLED)
  const hooks = useHookStatus()
  answers.hooks = ABSENT

  await hooks.uninstall()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'uninstall_hooks',
    'hook_status',
    'preview_hook_install',
  ])
  expect(hooks.status.value).toStrictEqual(ABSENT)
  expect(hooks.preview.value).toStrictEqual(PREVIEW)
  expect(hooks.busy.value).toBe(false)
})

it('keeps the reason a removal failed, and the hooks it left alone', async () => {
  const { useHookStatus } = await load(INSTALLED, {
    uninstall_hooks: { reason: 'The file changed while AYDY was writing it.' },
  })
  const hooks = useHookStatus()

  await hooks.uninstall()

  expect(hooks.failure.value).toStrictEqual({
    reason: 'The file changed while AYDY was writing it.',
  })
  expect(hooks.status.value).toStrictEqual(INSTALLED)
})
