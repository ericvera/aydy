import { invoke } from '@tauri-apps/api/core'
import type { Ref } from 'vue'
import { ref } from 'vue'

import type { TrackingStatus } from '../types/TrackingStatus'

const POLL_MS = 2000

// Module scoped, so the routing and every place that asks again read one
// answer.
const status = ref<TrackingStatus | null>(null)
let timer: ReturnType<typeof setTimeout> | null = null
let stopped = false

interface TrackingApi {
  status: Ref<TrackingStatus | null>
  read: () => Promise<void>
  watch: () => Promise<void>
  stop: () => void
}

/**
 * Whether the extension's session tracking is installed, polled while it is
 * not so the install screen clears by itself once the files appear.
 */
export const useTracking = (): TrackingApi => {
  const read = async (): Promise<void> => {
    try {
      status.value = await invoke<TrackingStatus>('tracking_status')
    } catch {
      // The last answer stays up: there is nothing the app could do about a
      // failed read, and the next poll or focus asks again.
    }
  }

  const stop = (): void => {
    stopped = true
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  const watch = async (): Promise<void> => {
    stopped = false
    await read()

    if (stopped) {
      return
    }

    if (status.value?.state === 'installed') {
      stop()

      return
    }

    if (timer === null) {
      timer = setTimeout(() => {
        timer = null
        void watch()
      }, POLL_MS)
    }
  }

  return { status, read, watch, stop }
}
