<script setup lang="ts">
/**
 * The session's row label.
 *
 * The title, branch, directory and placeholder chain runs in Rust and its last
 * link is `DEFAULT_SESSION_LABEL`, so the resolved label arrives with something
 * in it. Re-deriving any link here would mean two copies of the same
 * user-visible words, and changing the Rust one would leave a row still
 * rendering the old wording with both suites green.
 */
interface Props {
  label: string
  unread: boolean
}

const props = defineProps<Props>()
</script>

<template>
  <span
    class="truncate text-2"
    :class="props.unread ? 'title-unread' : 'title-read'"
  >
    {{ props.label }}
  </span>
</template>
