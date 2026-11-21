<script setup lang="ts">
/**
 * The on/off switch beside a setting.
 *
 * The track carries the brand colour when on, not the green the mock draws:
 * status colour is reserved for session status, and a control borrowing one is
 * how a coloured element stops being a reliable read of state.
 */
interface Props {
  checked: boolean
  /** What the switch is for, and how it is named to a screenreader. */
  label: string
}

const props = defineProps<Props>()

const emit = defineEmits<{ change: [boolean] }>()

const TRACK = {
  on: 'bg-brand-primary',
  off: 'bg-neutral-4',
}
</script>

<template>
  <button
    type="button"
    role="switch"
    class="relative h-5 w-7 shrink-0 rounded-full"
    :class="props.checked ? TRACK.on : TRACK.off"
    :aria-checked="props.checked"
    :aria-label="props.label"
    @click="emit('change', !props.checked)"
  >
    <span
      class="absolute top-1 size-4 rounded-full bg-neutral-7"
      :class="props.checked ? 'end-1' : 'start-1'"
    />
  </button>
</template>
