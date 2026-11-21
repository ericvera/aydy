<script setup lang="ts">
interface Props {
  variant: 'primary' | 'secondary'
  /**
   * `small` is the treatment for a button sitting beside a field or under a
   * settings row, where the regular one would outweigh the setting it acts on.
   */
  size?: 'regular' | 'small'
  disabled?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  size: 'regular',
  disabled: false,
})

const emit = defineEmits<{ press: [] }>()

// The two button treatments the popover has, kept here so no screen invents a
// third. Primary is the brand colour and carries the action the screen is
// about; secondary is a neutral chip beside it.
const VARIANTS: Record<Props['variant'], string> = {
  primary: 'bg-brand-primary text-neutral-7',
  secondary: 'border border-neutral-4 bg-neutral-3 text-neutral-7',
}

// A button that cannot be pressed has to look like one: the primary action is
// disabled for the seconds an install takes, and a brand-coloured button that
// does nothing reads as a click that was missed. Each variant keeps its own box
// so the row does not shift when the state changes.
const DISABLED: Record<Props['variant'], string> = {
  primary: 'bg-neutral-3 text-neutral-5',
  secondary: 'border border-neutral-4 bg-neutral-3 text-neutral-5',
}

const SIZES: Record<NonNullable<Props['size']>, string> = {
  regular: 'rounded px-4 py-2 text-2',
  small: 'rounded-small px-3 py-1 text-1',
}
</script>

<template>
  <button
    type="button"
    class="font-2"
    :class="[
      SIZES[props.size],
      (props.disabled ? DISABLED : VARIANTS)[props.variant],
    ]"
    :disabled="props.disabled"
    @click="emit('press')"
  >
    <slot />
  </button>
</template>
