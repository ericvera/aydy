<script setup lang="ts">
import { computed } from 'vue'

import type { WorkSignals } from '../../types/WorkSignals'
import { formatDuration } from '../../utils/formatDuration'

interface Props {
  signals: WorkSignals
}

const props = defineProps<Props>()

const quiet = computed(() =>
  props.signals.quietMs === null
    ? 'Quiet'
    : `Quiet ${formatDuration(props.signals.quietMs)}`,
)

const progress = computed(() => {
  const { step, eta, turnElapsedMs } = props.signals
  const parts: string[] = []

  if (step !== null) {
    parts.push(`${step.done}/${step.total}`)
  }

  if (eta !== null) {
    parts.push(
      eta.remainingMs >= 0
        ? `~${formatDuration(eta.remainingMs)} left`
        : `${formatDuration(-eta.remainingMs)} past ETA`,
    )
  }

  if (parts.length === 0 && turnElapsedMs !== null) {
    parts.push(`${formatDuration(turnElapsedMs)} elapsed`)
  }

  return parts.join(' · ')
})

// A fraction known only at runtime is the one thing utilities cannot express,
// so the fill width is the single bound inline style.
const fillWidth = computed(() =>
  props.signals.fractionPermille === null
    ? null
    : { width: `${props.signals.fractionPermille / 10}%` },
)
</script>

<template>
  <span class="flex items-center gap-3 pl-5 text-0 text-neutral-5">
    <span v-if="props.signals.stale" class="text-neutral-6">{{ quiet }}</span>
    <span
      v-else-if="fillWidth !== null"
      class="h-1 w-8 shrink-0 overflow-hidden rounded-full bg-neutral-3"
    >
      <span class="block h-full bg-neutral-6" :style="fillWidth" />
    </span>
    <span v-if="progress !== ''">{{ progress }}</span>
  </span>
</template>
