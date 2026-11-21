<script setup lang="ts">
/**
 * The listener moved itself to a free port and rewrote its own hooks.
 *
 * This is a notice and not an error: by the time it is shown the move has
 * already been made and the registered hooks already point at the new port.
 * Nothing here offers a port to choose — the port is not a setting, and asking
 * would put back the decision the app exists to make for itself.
 */
import type { PortMove } from '../../types/PortMove'
import Banner from './Banner.vue'

interface Props {
  move: PortMove
}

const props = defineProps<Props>()
</script>

<template>
  <Banner variant="notice">
    Port <strong class="font-3">{{ props.move.from }}</strong> was taken, so
    AYDY moved to <strong class="font-3">{{ props.move.to }}</strong> and
    updated its hooks. Sessions started before now report in again after their
    next message.
  </Banner>
</template>
