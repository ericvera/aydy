import { invoke } from '@tauri-apps/api/core'
import type { Ref } from 'vue'
import { ref } from 'vue'

import type { Settings } from '../types/Settings'
import type { SettingsChange } from '../types/SettingsChange'
import type { SettingsFailure } from '../types/SettingsFailure'
import { failureReason } from '../utils/failureReason'

// Module scoped, like the other two, so the pane and anything else that comes
// to read a setting read one answer.
const settings = ref<Settings | null>(null)
const failure = ref<SettingsFailure | null>(null)
const busy = ref(false)

interface SettingsApi {
  settings: Ref<Settings | null>
  failure: Ref<SettingsFailure | null>
  busy: Ref<boolean>
  read: () => Promise<void>
  update: (change: SettingsChange) => Promise<void>
}

const asFailure = (error: unknown): SettingsFailure => ({
  reason: failureReason(error),
})

/**
 * The settings the pane renders and edits.
 *
 * Every command answers with the settings as they now are, and that answer is
 * what the pane draws — never what it asked for. A change the app could not
 * make, such as launch at login the system refused, renders the value the app
 * is actually in rather than the one the user reached for.
 */
export const useSettings = (): SettingsApi => {
  const read = async (): Promise<void> => {
    busy.value = true

    try {
      settings.value = await invoke<Settings>('read_settings')
      failure.value = null
    } catch (error) {
      failure.value = asFailure(error)
    }

    busy.value = false
  }

  // A change that could not be saved is still in force — the store publishes
  // before it persists — so a failed command is followed by a read rather than
  // left showing the value the pane started from. The read leaves the failure
  // alone: its reason is the one the user needs, not a second one about the
  // read.
  const update = async (change: SettingsChange): Promise<void> => {
    busy.value = true

    try {
      settings.value = await invoke<Settings>('update_settings', { change })
      failure.value = null
    } catch (error) {
      failure.value = asFailure(error)

      try {
        settings.value = await invoke<Settings>('read_settings')
      } catch {
        // Nothing to add: the failure above is what went wrong.
      }
    }

    busy.value = false
  }

  return { settings, failure, busy, read, update }
}
