<script setup lang="ts">
/**
 * The screen AYDY shows whenever session tracking is not installed.
 *
 * The app installs nothing itself: the extension writes the hook script and
 * its hooks, and this screen clears by itself once they appear. Like every
 * screen before the list, its header carries no gear.
 */
import { ref } from 'vue'

import type { TrackingState } from '../../types/TrackingStatus'
import ActionButton from '../atoms/ActionButton.vue'
import FieldNote from '../atoms/FieldNote.vue'
import ScreenHeader from '../molecules/ScreenHeader.vue'

interface Props {
  state: Exclude<TrackingState, 'installed'>
}

const props = defineProps<Props>()

const showingSteps = ref(false)
</script>

<template>
  <div class="flex flex-col">
    <ScreenHeader title="Setup" />

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div
        v-if="props.state === 'hooksMissing'"
        class="px-5 pb-6 pt-1 text-center"
      >
        <p class="text-2 text-neutral-6">AYDY's hooks are missing</p>
        <p class="pt-1 text-1 text-neutral-5">
          The extension is installed, but its hooks are not in
          <strong class="font-3 text-neutral-6">~/.claude/settings.json</strong
          >, so sessions are not being recorded. Open any VS Code window and the
          extension puts them back.
        </p>
      </div>

      <template v-else>
        <div class="px-5 pb-6 pt-1 text-center">
          <p class="text-2 text-neutral-6">
            Install the AYDY extension in VS Code
          </p>
          <p class="pt-1 text-1 text-neutral-5">
            The extension tracks your Claude Code sessions, even while this app
            is closed. AYDY shows what it records.
          </p>
        </div>

        <ol
          v-if="showingSteps"
          class="list-decimal pb-4 pl-7 pr-5 text-1 text-neutral-6"
        >
          <li>In VS Code, open the Command Palette.</li>
          <li>
            Run
            <strong class="font-3 text-neutral-7"
              >Extensions: Install from VSIX…</strong
            >
          </li>
          <li>
            Choose
            <strong class="font-3 text-neutral-7">aydy-companion.vsix</strong>.
          </li>
          <li>Open any project window.</li>
        </ol>

        <template v-else>
          <div class="flex justify-center px-5 py-4">
            <ActionButton variant="primary" @press="showingSteps = true">
              Show me how
            </ActionButton>
          </div>
          <FieldNote class="px-5 text-center">
            Already installed? Open any VS Code window and this screen goes
            away.
          </FieldNote>
        </template>
      </template>
    </div>
  </div>
</template>
