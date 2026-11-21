<script setup lang="ts">
import { computed } from 'vue'

import IconButton from '../atoms/IconButton.vue'
import ScreenHeader from './ScreenHeader.vue'

interface Props {
  sessionCount: number
  unreadCount: number
}

const props = defineProps<Props>()

const emit = defineEmits<{ settings: [] }>()

// The unread half is the same number the menu bar icon carries, and both come
// from one snapshot computed in Rust so they cannot disagree.
const heading = computed((): string => {
  const { sessionCount, unreadCount } = props

  if (sessionCount === 0) {
    return 'No sessions'
  }

  const counted = `${sessionCount} ${sessionCount === 1 ? 'session' : 'sessions'}`

  return unreadCount === 0 ? counted : `${counted} · ${unreadCount} new`
})
</script>

<template>
  <ScreenHeader :title="heading">
    <!--
      The only way into settings, and it is on the list rather than on every
      screen: until session tracking is installed installing the extension is
      the only thing that can be done, so the screen that asks for it carries a header of their own with
      no gear in it. That is why the gear is passed into the shared header here
      rather than living inside it.
    -->
    <IconButton label="Settings" @press="emit('settings')">⚙</IconButton>
  </ScreenHeader>
</template>
