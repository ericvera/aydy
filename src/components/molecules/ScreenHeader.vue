<script setup lang="ts">
/**
 * The strip every screen in the popover opens with: its title on the left, and
 * whatever control that screen offers on the right.
 *
 * The control is passed in rather than built here, and that is the whole point
 * — **the gear belongs to the list header alone**. The screens that ask for the
 * hooks have one of these too, and they pass nothing into it, because until the
 * hooks exist installing them is the only thing that can be done. Anything
 * added to this component would appear on those screens as well.
 */
import ScreenTitle from '../atoms/ScreenTitle.vue'

interface Props {
  title: string
}

const props = defineProps<Props>()
</script>

<template>
  <header
    class="flex shrink-0 items-center justify-between gap-4 px-5 pb-3 pt-4"
  >
    <ScreenTitle>{{ props.title }}</ScreenTitle>
    <slot />
  </header>
</template>
