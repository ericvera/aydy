<script setup lang="ts">
import { computed } from 'vue'

import type { Session } from '../../types/Session'
import SessionTitle from '../atoms/SessionTitle.vue'
import StatusDot from '../atoms/StatusDot.vue'
import SessionProgressLine from './SessionProgressLine.vue'
import SessionRowDetail from './SessionRowDetail.vue'

interface Props {
  session: Session
}

const props = defineProps<Props>()

const emit = defineEmits<{ read: [id: string]; navigate: [id: string] }>()

const lineSignals = computed(() => {
  const { status, signals } = props.session

  if (status !== 'working' || signals === null) {
    return null
  }

  const hasSomethingToSay =
    signals.stale ||
    signals.step !== null ||
    signals.eta !== null ||
    signals.turnElapsedMs !== null

  return hasSomethingToSay ? signals : null
})

// Every row clears unread, jumpable or not: a session no VS Code window owns
// is cleared only by a click in the popover. Only a jumpable row asks
// to navigate, and navigating is what closes the popover.
const click = (): void => {
  emit('read', props.session.id)

  if (props.session.jumpable) {
    emit('navigate', props.session.id)
  }
}
</script>

<template>
  <button
    type="button"
    class="group/row row-divider flex w-full flex-col gap-1 px-5 py-2 text-left"
    :class="
      props.session.jumpable
        ? 'cursor-pointer hover:bg-neutral-2'
        : 'cursor-default'
    "
    @click="click"
  >
    <span class="flex w-full items-center gap-3">
      <StatusDot
        :status="props.session.status"
        :stale="props.session.signals?.stale ?? false"
      />
      <SessionTitle
        class="min-w-0 flex-1"
        :label="props.session.label"
        :unread="props.session.unread"
      />
    </span>
    <SessionProgressLine v-if="lineSignals" :signals="lineSignals" />
    <SessionRowDetail
      :status="props.session.status"
      :working-time-ms="props.session.workingTimeMs"
      :longest-turn-ms="props.session.longestTurnMs"
      :signals="props.session.signals"
    />
  </button>
</template>
