<script setup lang="ts">
/**
 * One setting: what it is on the left, the control that changes it on the
 * right, and anything that has to be said about it underneath.
 *
 * The note is a slot rather than a string so a row can explain itself in more
 * than one sentence, and what it says has to sit with the control it is
 * about.
 */
interface Props {
  label: string
}

const props = defineProps<Props>()
</script>

<template>
  <div class="py-2">
    <div class="flex items-center justify-between gap-4">
      <span class="text-2 text-neutral-7">{{ props.label }}</span>
      <slot name="control" />
    </div>
    <slot name="note" />
  </div>
</template>
