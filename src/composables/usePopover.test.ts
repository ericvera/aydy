import { expect, it, vi } from 'vitest'

const { invoked } = vi.hoisted(() => ({ invoked: [] as string[] }))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (command: string): Promise<void> => {
    invoked.push(command)
  },
}))

const load = async (): Promise<typeof import('./usePopover')> => {
  invoked.length = 0
  vi.resetModules()

  return import('./usePopover')
}

/**
 * The hide goes through Rust rather than the window API, so that every hide
 * passes the bookkeeping the tray icon's click-to-close depends on.
 */
it('closes the popover through Rust', async () => {
  const { usePopover } = await load()

  await usePopover().close()

  expect(invoked).toStrictEqual(['close_popover'])
})

it('quits through Rust', async () => {
  const { usePopover } = await load()

  await usePopover().quit()

  expect(invoked).toStrictEqual(['quit'])
})

it('does nothing until it is asked to', async () => {
  const { usePopover } = await load()

  usePopover()

  expect(invoked).toStrictEqual([])
})
