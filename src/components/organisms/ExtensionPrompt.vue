<script setup lang="ts">
/**
 * The line that tells a user the companion VS Code extension exists.
 *
 * It is shown only while no window is reporting to AYDY, and it goes away for
 * good once it is waved off: a companion nobody hears about does not get
 * installed, and a companion that asks every day is worse than one nobody
 * hears about. Everything it adds is an improvement on something that already
 * works, which is why this is a line under the list rather than anything that
 * interrupts.
 */
import FieldNote from '../atoms/FieldNote.vue'
import IconButton from '../atoms/IconButton.vue'

const emit = defineEmits<{ dismiss: [] }>()
</script>

<template>
  <section class="shrink-0 border-t border-neutral-3 px-5 py-3">
    <div class="flex items-start gap-3">
      <div class="min-w-0 flex-1">
        <p class="text-1 font-2 text-neutral-6">Add the VS Code extension</p>
        <FieldNote>
          It takes you to the tab a session is in, and keeps AYDY quiet about
          the session you are watching. No VS Code window is reporting to AYDY
          yet.
        </FieldNote>
      </div>
      <IconButton label="Dismiss" @press="emit('dismiss')">✕</IconButton>
    </div>
  </section>
</template>
