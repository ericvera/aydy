<script setup lang="ts">
/**
 * The exact change an install will make, offered before the user confirms it.
 *
 * Everything here comes from the plan the installer will commit, so what is
 * shown is what happens — including the block of JSON, which is the settings
 * file's `hooks` key exactly as it will be written.
 */
import type { HookPreview } from '../../types/HookPreview'
import SectionLabel from '../atoms/SectionLabel.vue'

interface Props {
  preview: HookPreview
}

const props = defineProps<Props>()
</script>

<template>
  <section class="px-5 pb-4 text-1 text-neutral-6">
    <SectionLabel class="pb-2">Hooks to be added</SectionLabel>
    <ul>
      <li v-for="hook in props.preview.added" :key="hook.event" class="text-0">
        {{ hook.event }} · {{ hook.kind }} · {{ hook.timeoutSeconds }}s timeout
      </li>
    </ul>

    <template v-if="props.preview.removed.length > 0">
      <SectionLabel class="pb-2 pt-4">Entries to be removed</SectionLabel>
      <ul>
        <li
          v-for="entry in props.preview.removed"
          :key="`${entry.event}:${entry.command ?? ''}`"
          class="text-0"
        >
          {{ entry.event
          }}<template v-if="entry.command !== null">
            — {{ entry.command }}</template
          >
        </li>
      </ul>
    </template>

    <SectionLabel class="pb-2 pt-4">
      The hooks block as it will be written
    </SectionLabel>
    <pre class="overflow-x-auto text-0 text-neutral-6">{{
      props.preview.hooksJson
    }}</pre>
  </section>
</template>
