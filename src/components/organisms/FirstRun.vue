<script setup lang="ts">
/**
 * The screen AYDY shows whenever its hooks are absent.
 *
 * It explains what will be written and asks, rather than writing and telling:
 * the file belongs to the user, and installing removes entries from it. Nothing
 * here routes to settings — installing the hooks is the only thing that can be
 * done until they exist.
 */
import { computed, ref } from 'vue'

import type { HookFailure } from '../../types/HookFailure'
import type { HookPreview } from '../../types/HookPreview'
import ActionButton from '../atoms/ActionButton.vue'
import Banner from '../molecules/Banner.vue'
import ScreenHeader from '../molecules/ScreenHeader.vue'
import HookChangePreview from './HookChangePreview.vue'

interface Props {
  settingsPath: string
  preview: HookPreview | null
  failure: HookFailure | null
  busy: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{ install: []; plan: []; reveal: [] }>()

const showingChange = ref(false)

// The number of hooks is the installer's own count of what it will register,
// never a number written into this copy.
const hooksAdded = computed((): string => {
  const count = props.preview?.hookCount ?? 0

  return `${count} ${count === 1 ? 'hook' : 'hooks'}`
})

const removals = computed(() => props.preview?.removed ?? [])

// Deleting entries from a file the user wrote is the most consequential thing
// this app does, so it is stated in full and before the button, not footnoted
// after it.
const removalLead = computed((): string => {
  const count = removals.value.length

  return count === 1
    ? 'Installing removes the hook below, which plays a sound when a session finishes or needs input.'
    : `Installing removes the ${count} hooks below, which play a sound when a session finishes or needs input.`
})

// What failed decides what retrying means. Without a plan nothing has named the
// entries an install would delete, so retrying plans again rather than writing:
// the planning is what fails when the app is asked before its listener has a
// port, and installing from there would take the user's hooks out having never
// said which ones.
const tryAgain = (): void => {
  if (props.preview === null) {
    emit('plan')

    return
  }

  emit('install')
}
</script>

<template>
  <div class="flex flex-col">
    <!-- The header carries nothing: no gear, because installing the hooks is
      the only thing that can be done until they exist. -->
    <ScreenHeader title="Setup" />

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div class="px-5 pb-6 pt-2 text-center">
        <p class="text-2 text-neutral-6">Track your Claude Code sessions</p>
        <p v-if="props.preview !== null" class="pt-2 text-1 text-neutral-5">
          AYDY adds <strong class="font-3">{{ hooksAdded }}</strong> to
          <strong class="font-3">{{ props.settingsPath }}</strong> so every
          project reports in. Everything else in that file is left untouched.
        </p>
        <p v-else class="pt-2 text-1 text-neutral-5">
          AYDY adds its hooks to
          <strong class="font-3">{{ props.settingsPath }}</strong> so every
          project reports in. Everything else in that file is left untouched.
        </p>
      </div>

      <Banner v-if="removals.length > 0" variant="warning">
        <p>
          {{ removalLead }} AYDY plays those sounds itself, so you do not hear
          both, and you can turn its sounds off afterwards.
        </p>
        <ul class="pt-2 text-0">
          <li
            v-for="entry in removals"
            :key="`${entry.event}:${entry.command}`"
          >
            {{ entry.event
            }}<template v-if="entry.command !== null">
              — {{ entry.command }}</template
            >
          </li>
        </ul>
      </Banner>

      <Banner v-if="props.failure !== null" variant="error">
        <strong class="font-3"
          >Nothing in {{ props.settingsPath }} was modified.</strong
        >
        {{ props.failure.reason }}
      </Banner>

      <div v-if="props.failure !== null" class="flex flex-wrap gap-3 px-5 py-4">
        <ActionButton
          variant="primary"
          :disabled="props.busy"
          @press="tryAgain"
        >
          Try again
        </ActionButton>
        <ActionButton variant="secondary" @press="emit('reveal')">
          Reveal file
        </ActionButton>
      </div>

      <template v-else-if="props.preview !== null">
        <HookChangePreview v-if="showingChange" :preview="props.preview" />

        <div class="flex flex-wrap gap-3 px-5 py-4">
          <ActionButton
            variant="primary"
            :disabled="props.busy"
            @press="emit('install')"
          >
            {{ props.busy ? 'Installing…' : 'Install hooks' }}
          </ActionButton>
          <ActionButton
            v-if="!showingChange"
            variant="secondary"
            @press="showingChange = true"
          >
            Show me the change first
          </ActionButton>
        </div>
      </template>
    </div>
  </div>
</template>
