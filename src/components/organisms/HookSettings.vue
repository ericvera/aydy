<script setup lang="ts">
/**
 * The hooks half of the settings pane: whether they are installed, whether
 * their events are arriving, and the two actions that change either.
 *
 * The two facts are stated separately and never collapsed into one. Hooks that
 * are registered and posting to a port nothing is listening on are installed
 * and silent, which is the one state here worth acting on — an app reporting
 * only that its hooks exist would say nothing at all about it.
 */
import { computed } from 'vue'

import type { HookFailure } from '../../types/HookFailure'
import type { HookStatus } from '../../types/HookStatus'
import ActionButton from '../atoms/ActionButton.vue'
import FieldNote from '../atoms/FieldNote.vue'
import SectionLabel from '../atoms/SectionLabel.vue'
import Banner from '../molecules/Banner.vue'

interface Props {
  status: HookStatus
  failure: HookFailure | null
  busy: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{ reinstall: []; remove: [] }>()

// The count is the installer's, counted from what is registered, so a sixth
// hook changes this line by itself.
const installed = computed((): string => {
  if (!props.status.installed) {
    return 'Not installed'
  }

  const count = props.status.hookCount

  return `Installed — ${count} ${count === 1 ? 'hook' : 'hooks'}`
})

const receiving = computed((): string =>
  props.status.receivingEvents ? 'Receiving events' : 'Not receiving events',
)

const silent = computed(
  (): boolean => props.status.installed && !props.status.receivingEvents,
)
</script>

<template>
  <section class="group-divider py-4">
    <div class="px-5">
      <SectionLabel>Hooks</SectionLabel>

      <p class="pt-2 text-2 text-neutral-7">{{ installed }}</p>
      <p class="text-2 text-neutral-7">{{ receiving }}</p>
    </div>

    <!--
      The one state on this screen the user can act on, so it gets the same
      treatment the list gives it — the stale notice above the session list is
      this same condition, and a warning there that is the dimmest line here
      would be two readings of one fact.
    -->
    <Banner v-if="silent" class="mt-3" variant="warning">
      The hooks are reporting to a port AYDY is not listening on, so it cannot
      see your sessions. <strong class="font-3">Reinstall</strong> points them
      back at it.
    </Banner>

    <div class="px-5">
      <div class="flex flex-wrap gap-3 pt-3">
        <ActionButton
          variant="secondary"
          size="small"
          :disabled="props.busy"
          @press="emit('reinstall')"
        >
          Reinstall
        </ActionButton>
        <ActionButton
          variant="secondary"
          size="small"
          :disabled="props.busy"
          @press="emit('remove')"
        >
          Remove hooks
        </ActionButton>
      </div>

      <FieldNote>
        Removing takes AYDY's hooks out of
        {{ props.status.settingsPath }} and puts back the sound hooks it
        replaced. AYDY keeps running and asks to install them again.
      </FieldNote>
    </div>

    <Banner v-if="props.failure !== null" class="mt-3" variant="error">
      <strong class="font-3"
        >Nothing in {{ props.status.settingsPath }} was modified.</strong
      >
      {{ props.failure.reason }}
    </Banner>
  </section>
</template>
