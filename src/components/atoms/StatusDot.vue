<script setup lang="ts">
import type { SessionStatus } from '../../types/SessionStatus'

interface Props {
  status: SessionStatus
}

const props = defineProps<Props>()

// Status colour is reserved for status, and this dot is the only thing in a
// row that carries one. Unread is weight and brightness, so the dot stays a
// reliable read of session state on its own.
const COLORS: Record<SessionStatus, string> = {
  needsInput: 'bg-status-needs-input',
  working: 'bg-status-working',
  done: 'bg-status-done',
  unknown: 'bg-status-idle',
}
</script>

<template>
  <span class="h-3 w-3 shrink-0 rounded-full" :class="COLORS[props.status]" />
</template>
