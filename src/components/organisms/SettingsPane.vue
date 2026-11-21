<script setup lang="ts">
/**
 * The settings pane, inside the popover and never a window of its own.
 *
 * It renders and sends; it does not decide. Every command answers with the
 * settings as they now are — so what is drawn here is what the app is in, not
 * what was asked for. The one thing this screen owes the user is the **reason**
 * a change did not take.
 *
 * There is no listener port here and there is not going to be one: choosing a
 * port is the decision the app exists to make for itself.
 */
import type { HookFailure } from '../../types/HookFailure'
import type { HookStatus } from '../../types/HookStatus'
import type { Settings } from '../../types/Settings'
import type { SettingsChange } from '../../types/SettingsChange'
import type { SettingsFailure } from '../../types/SettingsFailure'
import FieldNote from '../atoms/FieldNote.vue'
import IconButton from '../atoms/IconButton.vue'
import SectionLabel from '../atoms/SectionLabel.vue'
import Toggle from '../atoms/Toggle.vue'
import Banner from '../molecules/Banner.vue'
import ScreenHeader from '../molecules/ScreenHeader.vue'
import SettingRow from '../molecules/SettingRow.vue'
import HookSettings from './HookSettings.vue'

interface Props {
  settings: Settings | null
  failure: SettingsFailure | null
  hookStatus: HookStatus
  hookFailure: HookFailure | null
  hookBusy: boolean
  /** Whether any VS Code window's companion extension has registered. */
  extensionConnected: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{
  close: []
  update: [SettingsChange]
  reinstall: []
  removeHooks: []
}>()
</script>

<template>
  <div class="flex flex-col">
    <ScreenHeader title="Settings">
      <IconButton label="Close settings" @press="emit('close')">✕</IconButton>
    </ScreenHeader>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <Banner v-if="props.failure !== null" variant="error">
        {{ props.failure.reason }}
      </Banner>

      <template v-if="props.settings !== null">
        <section class="group-divider py-4">
          <div class="px-5">
            <SectionLabel>Sounds</SectionLabel>

            <div class="pt-2">
              <SettingRow label="When a session finishes">
                <template #control>
                  <Toggle
                    label="When a session finishes"
                    :checked="props.settings.finishedSound"
                    @change="emit('update', { finishedSound: $event })"
                  />
                </template>
              </SettingRow>

              <SettingRow label="When a session needs input">
                <template #control>
                  <Toggle
                    label="When a session needs input"
                    :checked="props.settings.needsInputSound"
                    @change="emit('update', { needsInputSound: $event })"
                  />
                </template>
              </SettingRow>
            </div>
          </div>
        </section>

        <section class="group-divider py-4">
          <div class="px-5">
            <SectionLabel>General</SectionLabel>

            <div class="pt-2">
              <SettingRow label="Launch at login">
                <template #control>
                  <Toggle
                    label="Launch at login"
                    :checked="props.settings.launchAtLogin"
                    @change="emit('update', { launchAtLogin: $event })"
                  />
                </template>
              </SettingRow>
            </div>
          </div>
        </section>

        <section class="group-divider py-4">
          <div class="px-5">
            <SectionLabel>Companion extension</SectionLabel>

            <div class="pt-2">
              <SettingRow label="VS Code extension">
                <template #control>
                  <span class="shrink-0 text-2 text-neutral-6">
                    {{
                      props.extensionConnected ? 'Connected' : 'Not connected'
                    }}
                  </span>
                </template>
                <template #note>
                  <FieldNote>
                    It tells AYDY which session you are looking at, so AYDY can
                    stay quiet about that one and take you back to it.
                    Everything else works without it.
                  </FieldNote>
                </template>
              </SettingRow>
            </div>
          </div>
        </section>

        <HookSettings
          :status="props.hookStatus"
          :failure="props.hookFailure"
          :busy="props.hookBusy"
          @reinstall="emit('reinstall')"
          @remove="emit('removeHooks')"
        />
      </template>
    </div>
  </div>
</template>
