<script setup lang="ts">
/**
 * The small uppercase title at the top of a screen — Settings, Setup, or what
 * the list currently is.
 *
 * One atom rather than the same class trio written at every screen, which is
 * how two of them drift apart. It is a size above `SectionLabel`, which heads a
 * block inside a screen rather than the screen itself.
 */
</script>

<template>
  <p class="text-1 font-3 uppercase text-neutral-5"><slot /></p>
</template>
