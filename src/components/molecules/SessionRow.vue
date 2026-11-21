<script setup lang="ts">
import type { Session } from '../../types/Session'
import SessionTitle from '../atoms/SessionTitle.vue'
import StatusDot from '../atoms/StatusDot.vue'
import SessionRowDetail from './SessionRowDetail.vue'

interface Props {
  session: Session
}

const props = defineProps<Props>()

const emit = defineEmits<{ read: [id: string]; navigate: [id: string] }>()

// Every row clears unread, jumpable or not: without the companion extension a
// click in the popover is the only way to clear it. Only a jumpable row asks
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
      <StatusDot :status="props.session.status" />
      <SessionTitle
        class="min-w-0 flex-1"
        :label="props.session.label"
        :unread="props.session.unread"
      />
      <span
        v-if="props.session.isCurrent"
        class="shrink-0 text-1 text-neutral-5"
      >
        you are here
      </span>
    </span>
    <SessionRowDetail
      :status="props.session.status"
      :working-time-ms="props.session.workingTimeMs"
      :longest-turn-ms="props.session.longestTurnMs"
    />
  </button>
</template>
