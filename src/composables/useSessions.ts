import { Channel, invoke } from '@tauri-apps/api/core'
import type { ShallowRef } from 'vue'
import { shallowRef } from 'vue'

import type { SessionList } from '../types/SessionList'

const EMPTY_LIST: SessionList = {
  groups: [],
  sessionCount: 0,
  unreadCount: 0,
  portMove: null,
  stale: null,
  extensionConnected: false,
}

// Module scoped, so every component reads the same list and the popover holds
// one subscription rather than one per caller.
const sessions = shallowRef<SessionList>(EMPTY_LIST)

interface Sessions {
  sessions: ShallowRef<SessionList>
  subscribe: () => Promise<void>
  markRead: (sessionId: string) => Promise<void>
}

/**
 * The session list, kept up to date by Rust.
 *
 * A channel can only be created on this side and handed over, so subscribing is
 * an invoke that passes one in. Rust keeps the most recent channel only, so
 * subscribing again after a reload replaces the subscription instead of adding
 * to it.
 */
export const useSessions = (): Sessions => {
  const subscribe = async (): Promise<void> => {
    const channel = new Channel<SessionList>()

    // Whole-object replacement against a shallow ref: the list arrives already
    // grouped and ordered, and deep reactivity over rows that can change ten
    // times a second costs more than it gives.
    channel.onmessage = (list): void => {
      sessions.value = list
    }

    await invoke('subscribe_sessions', { channel })
  }

  // Nothing is updated optimistically: the registry owns unread, and the next
  // observation pushes the cleared row back within a tenth of a second.
  const markRead = async (sessionId: string): Promise<void> => {
    await invoke('mark_read', { sessionId })
  }

  return { sessions, subscribe, markRead }
}
