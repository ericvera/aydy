import { invoke } from '@tauri-apps/api/core'
import type { Ref } from 'vue'
import { ref } from 'vue'

import type { ExtensionStatus } from '../types/ExtensionStatus'

/** Where the popover remembers that the prompt was waved off. */
const DISMISSED_KEY = 'aydy.extension-prompt-dismissed'

const DISMISSED = 'yes'

/**
 * Whether the user has already waved the prompt away.
 *
 * Kept by the webview rather than by the app: it is a preference about this
 * interface and nothing else reads it, and storage that is unavailable leaves
 * the prompt showing, which is the recoverable direction.
 */
const wasDismissed = (): boolean => {
  try {
    return window.localStorage.getItem(DISMISSED_KEY) === DISMISSED
  } catch {
    return false
  }
}

// Module scoped, like the other composables, so the pane and anything else
// that asks read one answer.
const connected = ref(false)
const windowCount = ref(0)
const dismissed = ref(wasDismissed())

interface ExtensionApi {
  connected: Ref<boolean>
  windowCount: Ref<number>
  read: () => Promise<void>
  dismissed: Ref<boolean>
  dismiss: () => void
}

/**
 * Whether the companion VS Code extension is connected, as the settings pane
 * reports it.
 *
 * Read when the pane opens rather than kept up to date: windows open and close
 * all day, nothing announces a window closing, and the only moment the answer
 * has to be right is the moment someone is looking at it.
 */
export const useExtension = (): ExtensionApi => {
  const read = async (): Promise<void> => {
    try {
      const status = await invoke<ExtensionStatus>('extension_status')
      connected.value = status.connected
      windowCount.value = status.windowCount
    } catch {
      // Connected is a claim that something is working, and a read that never
      // answered is not evidence for it.
      connected.value = false
      windowCount.value = 0
    }
  }

  // Offered once and then never again: a companion nobody hears about does
  // not get installed, and one that asks every day is worse than that.
  const dismiss = (): void => {
    dismissed.value = true

    try {
      window.localStorage.setItem(DISMISSED_KEY, DISMISSED)
    } catch {
      // Storage that cannot be written leaves the prompt to be waved off
      // again next time, which is the whole of the cost.
    }
  }

  return { connected, windowCount, read, dismissed, dismiss }
}
