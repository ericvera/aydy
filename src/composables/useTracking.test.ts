import { afterEach, beforeEach, expect, it, vi } from 'vitest'

interface Answers {
  status: unknown
  fails: boolean
}

const { invoked, answers } = vi.hoisted(
  (): { invoked: string[]; answers: Answers } => ({
    invoked: [],
    answers: { status: { state: 'notInstalled' }, fails: false },
  }),
)

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (command: string): Promise<unknown> => {
    invoked.push(command)

    if (answers.fails) {
      throw new Error('no answer')
    }

    return answers.status
  },
}))

/**
 * A fresh module, since the state and the timer are module scoped.
 */
const load = async (
  status: unknown,
): Promise<ReturnType<(typeof import('./useTracking'))['useTracking']>> => {
  invoked.length = 0
  answers.status = status
  answers.fails = false
  vi.resetModules()

  const { useTracking } = await import('./useTracking')

  return useTracking()
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

it('knows nothing until it is asked', async () => {
  const tracking = await load({ state: 'installed' })

  expect(tracking.status.value).toBeNull()
  expect(invoked).toStrictEqual([])
})

it('sets the status from what the app answers', async () => {
  const tracking = await load({ state: 'hooksMissing' })

  await tracking.read()

  expect(invoked).toStrictEqual(['tracking_status'])
  expect(tracking.status.value).toStrictEqual({ state: 'hooksMissing' })
})

it('keeps polling while tracking is not installed and stops once it is', async () => {
  const tracking = await load({ state: 'notInstalled' })

  await tracking.watch()
  await vi.advanceTimersByTimeAsync(2000)
  await vi.advanceTimersByTimeAsync(2000)

  expect(invoked).toHaveLength(3)

  answers.status = { state: 'installed' }
  await vi.advanceTimersByTimeAsync(2000)

  expect(tracking.status.value).toStrictEqual({ state: 'installed' })
  expect(invoked).toHaveLength(4)

  await vi.advanceTimersByTimeAsync(10_000)

  expect(invoked).toHaveLength(4)
})

it('does not poll at all once tracking is installed', async () => {
  const tracking = await load({ state: 'installed' })

  await tracking.watch()
  await vi.advanceTimersByTimeAsync(10_000)

  expect(invoked).toHaveLength(1)
})

/**
 * Every focus of the popover asks again, and that must not stack a second
 * poll on top of the one already running.
 */
it('does not double the polling when watched twice', async () => {
  const tracking = await load({ state: 'notInstalled' })

  await tracking.watch()
  await tracking.watch()
  invoked.length = 0

  await vi.advanceTimersByTimeAsync(2000)

  expect(invoked).toHaveLength(1)
})

it('stops polling when it is told to', async () => {
  const tracking = await load({ state: 'notInstalled' })

  await tracking.watch()
  tracking.stop()
  await vi.advanceTimersByTimeAsync(10_000)

  expect(invoked).toHaveLength(1)
})

it('stays stopped when stopped while a read is in flight', async () => {
  const tracking = await load({ state: 'notInstalled' })

  const watching = tracking.watch()
  tracking.stop()
  await watching
  await vi.advanceTimersByTimeAsync(10_000)

  expect(invoked).toHaveLength(1)
})

it('keeps the last answer when a read fails', async () => {
  const tracking = await load({ state: 'hooksMissing' })
  await tracking.read()

  answers.fails = true
  await tracking.read()

  expect(tracking.status.value).toStrictEqual({ state: 'hooksMissing' })
})
