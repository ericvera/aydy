<script setup lang="ts">
import type { SessionStatus } from '../../types/SessionStatus'

interface Props {
  status: SessionStatus
  stale?: boolean
}

const props = withDefaults(defineProps<Props>(), { stale: false })

// Status colour is reserved for status, and this dot is the only thing in a
// row that carries one. Unread is weight and brightness, so the dot stays a
// reliable read of session state on its own.
const COLORS: Record<SessionStatus, string> = {
  needsInput: 'bg-status-needs-input',
  working: 'bg-status-working',
  done: 'bg-status-done',
  unknown: 'bg-status-idle',
}

// The dot still owns the colour: a stale session's status stays Working, so
// it keeps the working hue and only loses its fill. Stale only ever arrives
// with status `working`.
const HOLLOW = 'box-border border border-status-working'
</script>

<template>
  <span
    class="h-3 w-3 shrink-0 rounded-full"
    :class="props.stale ? HOLLOW : COLORS[props.status]"
  />
</template>
