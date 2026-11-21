<script setup lang="ts">
import { computed } from 'vue'

import IconButton from '../atoms/IconButton.vue'
import ScreenHeader from './ScreenHeader.vue'

interface Props {
  sessionCount: number
  unreadCount: number
  /** Whether the list below it is a stale one rather than a current count. */
  stale: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{ settings: [] }>()

// The unread half is the same number the menu bar icon carries, and both come
// from one snapshot computed in Rust so they cannot disagree.
//
// A stale list has no count to head: the registry goes on reaping sessions
// while events cannot reach it, so the live count drains away underneath a
// notice stating what was last seen — No sessions above Last seen 3 sessions.
// The state the app is in is what the header says instead.
const heading = computed((): string => {
  const { sessionCount, unreadCount } = props

  if (props.stale) {
    return 'Reconnecting'
  }

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
      screen: until the hooks exist installing them is the only thing that can
      be done, so the screens that ask for them carry a header of their own with
      no gear in it. That is why the gear is passed into the shared header here
      rather than living inside it.
    -->
    <IconButton label="Settings" @press="emit('settings')">⚙</IconButton>
  </ScreenHeader>
</template>
