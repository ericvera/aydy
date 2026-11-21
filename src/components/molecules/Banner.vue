<script setup lang="ts">
interface Props {
  variant: 'notice' | 'warning' | 'error'
}

const props = defineProps<Props>()

// A tinted band across the popover, one tint per severity. The status tints
// exist for exactly this; the neutral one is for something that has already
// resolved itself and is being reported rather than raised.
const VARIANTS: Record<Props['variant'], string> = {
  notice: 'bg-neutral-3',
  warning: 'bg-statustransparent-needs-input',
  error: 'bg-statustransparent-error',
}
</script>

<template>
  <div class="px-5 py-4 text-1 text-neutral-7" :class="VARIANTS[props.variant]">
    <slot />
  </div>
</template>
