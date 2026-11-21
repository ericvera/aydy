<script setup lang="ts">
const emit = defineEmits<{ quit: [] }>()
</script>

<template>
  <footer class="shrink-0 border-t border-neutral-3 px-5 py-3">
    <button type="button" class="text-1 text-neutral-6" @click="emit('quit')">
      Quit
    </button>
  </footer>
</template>
