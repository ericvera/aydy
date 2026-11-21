import { invoke } from '@tauri-apps/api/core'
import type { Ref } from 'vue'
import { ref } from 'vue'

import type { HookFailure } from '../types/HookFailure'
import type { HookPreview } from '../types/HookPreview'
import type { HookStatus } from '../types/HookStatus'
import { failureReason } from '../utils/failureReason'

// Module scoped, so the screen that installs the hooks and the routing that
// decides whether to show it read one answer.
const status = ref<HookStatus | null>(null)
const preview = ref<HookPreview | null>(null)
const failure = ref<HookFailure | null>(null)
const busy = ref(false)

interface HookStatusApi {
  status: Ref<HookStatus | null>
  preview: Ref<HookPreview | null>
  failure: Ref<HookFailure | null>
  busy: Ref<boolean>
  refresh: () => Promise<boolean>
  plan: () => Promise<void>
  install: () => Promise<void>
  reinstall: () => Promise<void>
  uninstall: () => Promise<void>
  revealSettingsFile: () => Promise<void>
}

// A rejected command carries whatever Rust returned as its error.
const asFailure = (error: unknown): HookFailure => ({
  reason: failureReason(error),
})

/**
 * Whether AYDY's hooks are registered, what installing them would change, and
 * the two actions the first-run screen offers.
 *
 * The status is read from the settings file rather than remembered, because the
 * file is the user's: hooks removed by hand put the first-run screen back in
 * front of them, and the count and the removals the screen states come from the
 * installer's own plan rather than from copy.
 */
export const useHookStatus = (): HookStatusApi => {
  // The preview is only asked for while the hooks are absent: it is the
  // first-run screen's content, and planning an install is a file read.
  const refresh = async (): Promise<boolean> => {
    try {
      const next = await invoke<HookStatus>('hook_status')
      status.value = next
      preview.value = next.installed
        ? null
        : await invoke<HookPreview>('preview_hook_install')

      return true
    } catch (error) {
      failure.value = asFailure(error)

      return false
    }
  }

  // Reading the file again after a read or a plan failed, which is what
  // retrying means when there is no plan to install from: the plan is what
  // names the entries an install deletes.
  //
  // The failure is cleared by the read that succeeds rather than by the attempt
  // — the banner and the reason stay up while the retry runs, and a retry that
  // fails again replaces them rather than blanking the screen in between.
  const plan = async (): Promise<void> => {
    busy.value = true

    if (await refresh()) {
      failure.value = null
    }

    busy.value = false
  }

  // Nothing is assumed about the outcome: the status is read back from the file
  // afterwards either way, so a failed install leaves the first-run screen up
  // because the hooks really are still absent, and a removal that worked puts
  // that screen back without the app going anywhere.
  const change = async (command: string): Promise<void> => {
    busy.value = true
    failure.value = null

    try {
      await invoke(command)
    } catch (error) {
      failure.value = asFailure(error)
    }

    await refresh()
    busy.value = false
  }

  const install = (): Promise<void> => change('install_hooks')

  const reinstall = (): Promise<void> => change('reinstall_hooks')

  const uninstall = (): Promise<void> => change('uninstall_hooks')

  const revealSettingsFile = async (): Promise<void> => {
    try {
      await invoke('reveal_settings_file')
    } catch (error) {
      failure.value = asFailure(error)
    }
  }

  return {
    status,
    preview,
    failure,
    busy,
    refresh,
    plan,
    install,
    reinstall,
    uninstall,
    revealSettingsFile,
  }
}
