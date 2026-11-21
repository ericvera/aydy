<script setup lang="ts">
/**
 * A jump that did not do what it was asked, said out loud.
 *
 * A click that quietly did nothing would look like an app that ignored it.
 * The two failures are given different weights: a jump no window answered
 * most likely means the window is gone, while a tab that opened is known to be
 * wrong and asks the user to act.
 */
import type { JumpOutcome } from '../../types/JumpOutcome'
import IconButton from '../atoms/IconButton.vue'
import Banner from './Banner.vue'

interface Props {
  failure: Extract<JumpOutcome, 'unanswered' | 'duplicated'>
}

const props = defineProps<Props>()

const emit = defineEmits<{ dismiss: [] }>()
</script>

<template>
  <Banner :variant="props.failure === 'duplicated' ? 'error' : 'warning'">
    <span class="flex items-start gap-3">
      <span v-if="props.failure === 'duplicated'" class="min-w-0 flex-1">
        <strong class="font-3">A new Claude tab opened.</strong> Close it
        without typing in it — it may be a second copy of that session.
      </span>
      <span v-else class="min-w-0 flex-1">
        <strong class="font-3">No VS Code window took the jump.</strong> The
        window that had this session may be closed.
      </span>
      <IconButton label="Dismiss" @press="emit('dismiss')">✕</IconButton>
    </span>
  </Banner>
</template>
