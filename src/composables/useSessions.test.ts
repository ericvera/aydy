import { expect, it, vi } from 'vitest'

import type { SessionList } from '../types/SessionList'

interface FakeChannel {
  onmessage: (list: SessionList) => void
}

interface Invocation {
  command: string
  args: Record<string, unknown> | undefined
}

const { channels, invoked } = vi.hoisted(() => ({
  channels: [] as FakeChannel[],
  invoked: [] as {
    command: string
    args: Record<string, unknown> | undefined
  }[],
}))

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: (list: SessionList) => void = () => {}

    constructor() {
      channels.push(this)
    }
  },
  invoke: async (
    command: string,
    args?: Record<string, unknown>,
  ): Promise<void> => {
    invoked.push({ command, args })
  },
}))

const list = (sessionCount: number, unreadCount: number): SessionList => ({
  groups: [],
  sessionCount,
  unreadCount,
  portMove: null,
  stale: null,
  extensionConnected: false,
})

/**
 * A fresh copy of the composable, since its state is module scoped on purpose.
 */
const load = async (): Promise<typeof import('./useSessions')> => {
  channels.length = 0
  invoked.length = 0
  vi.resetModules()

  return import('./useSessions')
}

it('starts with an empty list', async () => {
  const { useSessions } = await load()

  expect(useSessions().sessions.value).toStrictEqual(list(0, 0))
})

it('subscribes by handing a channel to Rust', async () => {
  const { useSessions } = await load()

  await useSessions().subscribe()

  expect(invoked).toHaveLength(1)
  expect(invoked[0]?.command).toBe('subscribe_sessions')
  expect(invoked[0]?.args?.['channel']).toBe(channels[0])
})

/**
 * Without the companion extension a click in the popover is the only way to
 * clear unread, so this is the command the whole unread story rests on.
 */
it('marks a session read by name', async () => {
  const { useSessions } = await load()

  await useSessions().markRead('0d4b9a1c')

  expect(invoked).toStrictEqual([
    { command: 'mark_read', args: { sessionId: '0d4b9a1c' } },
  ] satisfies Invocation[])
})

/**
 * Unread is the registry's, not the interface's. Clearing it locally would let
 * the popover disagree with the menu bar icon for as long as it took the next
 * snapshot to arrive, and would hide a command that never landed.
 */
it('leaves the list alone until Rust says the session is read', async () => {
  const { useSessions } = await load()
  const { sessions, subscribe, markRead } = useSessions()

  await subscribe()
  channels[0]?.onmessage(list(1, 1))
  await markRead('0d4b9a1c')

  expect(sessions.value.unreadCount).toBe(1)
})

it('replaces the whole list when a snapshot arrives', async () => {
  const { useSessions } = await load()
  const { sessions, subscribe } = useSessions()

  await subscribe()
  const before = sessions.value
  channels[0]?.onmessage(list(6, 2))

  expect(sessions.value).toStrictEqual(list(6, 2))
  expect(sessions.value).not.toBe(before)
})

it('shares one list between callers', async () => {
  const { useSessions } = await load()
  const first = useSessions()
  const second = useSessions()

  await first.subscribe()
  channels[0]?.onmessage(list(3, 1))

  expect(second.sessions.value.sessionCount).toBe(3)
})

it('subscribing again replaces the channel Rust writes to', async () => {
  const { useSessions } = await load()
  const { sessions, subscribe } = useSessions()

  await subscribe()
  await subscribe()
  channels[1]?.onmessage(list(2, 0))

  expect(channels).toHaveLength(2)
  expect(sessions.value.sessionCount).toBe(2)
})
