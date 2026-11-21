<script setup lang="ts">
import { computed } from 'vue'

import type { SessionGroupKind } from '../../types/SessionGroupKind'

interface Props {
  kind: SessionGroupKind
  repo: string | null
  branch: string | null
}

const props = defineProps<Props>()

const label = computed((): string => {
  if (props.kind === 'notJumpable') {
    return 'Not jumpable'
  }

  if (props.kind === 'unidentified' || props.repo === null) {
    return 'Not yet identified'
  }

  return props.branch === null ? props.repo : `${props.repo} · ${props.branch}`
})
</script>

<template>
  <p class="px-5 pb-2 pt-4 text-0 font-3 uppercase text-neutral-6">
    {{ label }}
  </p>
</template>
