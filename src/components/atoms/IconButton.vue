<script setup lang="ts">
/**
 * The bare glyph button in a screen header: the gear into settings, the ✕ back
 * out of it.
 *
 * One atom rather than the same class trio written at each header. The glyph is
 * the slot and carries no meaning to anything that cannot see it, so the label
 * is required — it is what a screen reader reads and what every test reaches
 * the control by.
 */
interface Props {
  label: string
}

const props = defineProps<Props>()

const emit = defineEmits<{ press: [] }>()
</script>

<template>
  <button
    type="button"
    class="rounded-small px-2 py-1 text-2 text-neutral-6"
    :aria-label="props.label"
    @click="emit('press')"
  >
    <slot />
  </button>
</template>
