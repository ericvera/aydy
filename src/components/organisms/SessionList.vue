<script setup lang="ts">
import type { SessionGroup } from '../../types/SessionGroup'
import GroupHeader from '../molecules/GroupHeader.vue'
import SessionRow from '../molecules/SessionRow.vue'

interface Props {
  groups: SessionGroup[]
}

const props = defineProps<Props>()

const emit = defineEmits<{ read: [id: string]; navigate: [id: string] }>()

// Grouping and ordering are done in Rust, so the list renders what it is
// given. Deriving either here is how the popover and the menu bar icon start
// to disagree.
//
// The key has to be the group's identity rather than its position, or a group
// arriving above another remounts every row below it.
const keyOf = (group: SessionGroup): string =>
  `${group.kind}:${group.repo ?? ''}:${group.branch ?? ''}`

// `group/list` on the root is the outer half of the row hover reveal: a row's
// detail line takes its space as soon as the pointer is anywhere in the list,
// and only the row under the pointer paints one. Reserving it per row instead
// is what made every row below the pointer jump each time the pointer crossed a
// row boundary. The list's own box is sized by the popover rather than by its
// contents, so whether the pointer is inside it is not something the reveal can
// change — there is no loop here to close.
//
// The comment belongs here rather than over the element: a comment beside the
// root would make this a fragment, and the popover's `overflow-y-auto` is
// passed in as a fallthrough attribute that only a single root receives.
</script>

<template>
  <div class="group/list">
    <section
      v-for="group in props.groups"
      :key="keyOf(group)"
      class="group-divider"
    >
      <GroupHeader
        :kind="group.kind"
        :repo="group.repo"
        :branch="group.branch"
      />
      <SessionRow
        v-for="session in group.sessions"
        :key="session.id"
        :session="session"
        @read="emit('read', $event)"
        @navigate="emit('navigate', $event)"
      />
    </section>
  </div>
</template>
