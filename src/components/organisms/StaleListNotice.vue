<script setup lang="ts">
/**
 * What the popover shows in place of the list while the app cannot receive
 * events.
 *
 * A list that is not being updated must not be presented as current, and an
 * empty one would read as "nothing is running" — so the list is replaced by
 * what was last seen and when.
 */
import { computed } from 'vue'

import { formatDuration } from '../../utils/formatDuration'
import Banner from '../molecules/Banner.vue'

interface Props {
  sessionCount: number
  lastSeenAgoMs: number
}

const props = defineProps<Props>()

const seen = computed((): string => {
  if (props.sessionCount === 0) {
    return 'no sessions'
  }

  return `${props.sessionCount} ${props.sessionCount === 1 ? 'session' : 'sessions'}`
})
</script>

<template>
  <div>
    <Banner variant="warning">
      AYDY is not receiving session events, so this list is stale.
    </Banner>
    <p class="px-5 py-6 text-center text-1 text-neutral-5">
      Last seen {{ seen }}, {{ formatDuration(props.lastSeenAgoMs) }} ago.
    </p>
  </div>
</template>
