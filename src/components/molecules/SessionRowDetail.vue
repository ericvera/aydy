<script setup lang="ts">
/**
 * The line a row reveals under its title while the pointer is over it.
 *
 * The reveal is the browser's, not the component's: this line is always in the
 * DOM and revealed by `:hover` alone. A stored hover flag is never told that
 * the popover was hidden or that the list reordered under a still pointer, so
 * it leaves a resting row showing its detail; `:hover` is re-resolved against
 * the real pointer after both.
 *
 * **It reveals without moving anything.** Keeping the line out of the layout
 * until its own row was hovered made every row below the pointer jump by the
 * line's height each time the pointer crossed a row boundary, and moving the
 * mouse down the list was one jump per row. So the space is taken as soon as
 * the pointer is anywhere in the list, and hovering a row only paints a line
 * that is already standing there: measured in a browser at the popover's own
 * width, a row is 27px tall at rest and 45px with the pointer in the list, and
 * no row moves by a pixel as the pointer travels between them.
 *
 * Reserving it in every row all the time was the other way to stand still, and
 * it is not the one taken: a blank line under every title turns the one-line
 * row four rounds of mock review argued down into a two-line row for as long
 * as the popover is open. With the pointer off the list, the list is exactly
 * what was drawn.
 */
import { computed } from 'vue'

import type { SessionStatus } from '../../types/SessionStatus'
import type { WorkSignals } from '../../types/WorkSignals'
import { formatDuration } from '../../utils/formatDuration'
import { statusText } from '../../utils/statusText'

interface Props {
  status: SessionStatus
  workingTimeMs: number
  longestTurnMs: number
  signals: WorkSignals | null
}

const props = defineProps<Props>()

const signalParts = computed(() => {
  const signals = props.signals

  if (props.status !== 'working' || signals === null) {
    return []
  }

  const parts: { text: string; quiet: boolean }[] = []

  if (signals.stale) {
    const duration =
      signals.quietMs === null ? '' : ` ${formatDuration(signals.quietMs)}`

    parts.push({
      text: `Quiet${duration} · nothing written to the transcript`,
      quiet: true,
    })
  }

  if (signals.turnElapsedMs !== null) {
    parts.push({
      text: `This turn ${formatDuration(signals.turnElapsedMs)}`,
      quiet: false,
    })
  }

  parts.push({
    text:
      signals.eta === null
        ? 'No ETA reported'
        : `ETA reported ${formatDuration(signals.eta.reportedAgoMs)} ago`,
    quiet: false,
  })

  return parts
})
</script>

<template>
  <span
    class="hidden invisible flex-wrap gap-3 text-0 text-neutral-5 group-hover/list:flex group-hover/row:visible"
  >
    <span>{{ statusText(props.status) }}</span>
    <span
      v-for="part in signalParts"
      :key="part.text"
      :class="{ 'text-neutral-6': part.quiet }"
    >
      {{ part.text }}
    </span>
    <span>Working time {{ formatDuration(props.workingTimeMs) }}</span>
    <span>Longest turn {{ formatDuration(props.longestTurnMs) }}</span>
  </span>
</template>
