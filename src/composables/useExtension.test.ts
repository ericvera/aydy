import { expect, it, vi } from 'vitest'

interface Answers {
  status: unknown
  fails: boolean
}

const { invoked, answers } = vi.hoisted(
  (): { invoked: string[]; answers: Answers } => ({
    invoked: [],
    answers: { status: { connected: false, windowCount: 0 }, fails: false },
  }),
)

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (command: string): Promise<unknown> => {
    invoked.push(command)

    if (answers.fails) {
      throw new Error('no state')
    }

    return answers.status
  },
}))

/**
 * A fresh module, since the state is module scoped so that every caller reads
 * one answer.
 */
const load = async (
  status: unknown = { connected: false, windowCount: 0 },
  fails = false,
): Promise<ReturnType<(typeof import('./useExtension'))['useExtension']>> => {
  invoked.length = 0
  answers.status = status
  answers.fails = fails
  vi.resetModules()

  const { useExtension } = await import('./useExtension')

  return useExtension()
}

it('reports the extension as not connected until it is asked', async () => {
  const extension = await load()

  expect(extension.connected.value).toBe(false)
  expect(invoked).toStrictEqual([])
})

it('reports the extension as connected once a window has registered', async () => {
  const extension = await load({ connected: true, windowCount: 1 })

  await extension.read()

  expect(invoked).toStrictEqual(['extension_status'])
  expect(extension.connected.value).toBe(true)
  expect(extension.windowCount.value).toBe(1)
})

it('counts every registered window rather than only the first', async () => {
  const extension = await load({ connected: true, windowCount: 3 })

  await extension.read()

  expect(extension.windowCount.value).toBe(3)
})

/**
 * The windows were closed while the popover sat there. Nothing said so, which
 * is why the pane asks again every time it is opened.
 */
it('reports the extension as not connected again once every window has closed', async () => {
  const extension = await load({ connected: true, windowCount: 1 })
  await extension.read()

  answers.status = { connected: false, windowCount: 0 }
  await extension.read()

  expect(extension.connected.value).toBe(false)
  expect(extension.windowCount.value).toBe(0)
})

/**
 * Connected is a claim that something is working. A read that never answered is
 * not evidence for it, and leaving the last answer up would have the pane say
 * the extension is there on the strength of a question nobody answered.
 */
it('does not keep claiming a connection it could not confirm', async () => {
  const extension = await load({ connected: true, windowCount: 2 })
  await extension.read()

  answers.fails = true
  await extension.read()

  expect(extension.connected.value).toBe(false)
  expect(extension.windowCount.value).toBe(0)
})

/**
 * A companion nobody hears about does not get installed, and one that asks
 * every day is worse than that — so the offer is remembered as waved off by
 * the webview, which outlives the app being restarted.
 */
it('remembers that the offer of the extension was waved off', async () => {
  window.localStorage.clear()
  const extension = await load()
  expect(extension.dismissed.value).toBe(false)

  extension.dismiss()

  expect(extension.dismissed.value).toBe(true)
  expect((await load()).dismissed.value).toBe(true)
})

/**
 * Storage that cannot be read leaves the prompt showing, which costs a line in
 * the popover — the recoverable direction.
 */
it('offers the extension when nothing can be remembered at all', async () => {
  window.localStorage.clear()
  const storage = vi
    .spyOn(window.localStorage, 'getItem')
    .mockImplementation(() => {
      throw new Error('storage is not available')
    })
  const setting = vi
    .spyOn(window.localStorage, 'setItem')
    .mockImplementation(() => {
      throw new Error('storage is not available')
    })

  const extension = await load()

  expect(extension.dismissed.value).toBe(false)
  expect(() => {
    extension.dismiss()
  }).not.toThrow()
  expect(extension.dismissed.value).toBe(true)

  storage.mockRestore()
  setting.mockRestore()
})
