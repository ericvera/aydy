<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'

import JumpFailureNotice from './components/molecules/JumpFailureNotice.vue'
import ListFooter from './components/molecules/ListFooter.vue'
import PopoverHeader from './components/molecules/PopoverHeader.vue'
import EmptyState from './components/organisms/EmptyState.vue'
import InstallExtension from './components/organisms/InstallExtension.vue'
import SessionList from './components/organisms/SessionList.vue'
import SettingsPane from './components/organisms/SettingsPane.vue'
import { useJump } from './composables/useJump'
import { usePopover } from './composables/usePopover'
import { useSessions } from './composables/useSessions'
import { useSettings } from './composables/useSettings'
import { useTracking } from './composables/useTracking'
import type { SessionGroup } from './types/SessionGroup'
import type { SettingsChange } from './types/SettingsChange'

const { sessions, subscribe, markRead } = useSessions()
const { close, quit } = usePopover()
const {
  status: tracking,
  watch: watchTracking,
  stop: stopTracking,
} = useTracking()
const {
  settings,
  failure: settingsFailure,
  read: readSettings,
  update,
} = useSettings()
const { failure: jumpFailure, jump, dismiss: dismissJump } = useJump()

// Settings is a screen inside the popover rather than a window, and it is
// reachable only from the list: the install screen carries a header of its own
// but no gear in it, so there is nothing to reach settings from until tracking
// is installed.
const showingSettings = ref(false)

const recheckTracking = (): void => {
  void watchTracking()
}

// Polling stops once tracking is installed, and the popover's DOM is never
// unmounted, so without this an extension uninstalled later would leave a
// frozen list behind. The popover hides on focus loss, so it gains focus each
// time it opens.
onMounted(() => {
  void subscribe()
  recheckTracking()
  window.addEventListener('focus', recheckTracking)
})

onUnmounted(() => {
  window.removeEventListener('focus', recheckTracking)
  stopTracking()
})

// The settings are read when the pane opens rather than at startup, so the pane
// shows what the store holds at the moment the user asked to see it.
const openSettings = (): void => {
  showingSettings.value = true
  void readSettings()
  recheckTracking()
}

const changeSettings = (change: SettingsChange): void => {
  void update(change)
}

const groups = computed((): SessionGroup[] => sessions.value.groups)

const read = (sessionId: string): void => {
  void markRead(sessionId)
}

// A click that navigates closes the popover; a click that only marks a session
// read leaves it open, since the user is still reading the list — and a jump
// that could not find a window to raise is one of those, which is why the
// popover is closed on the answer rather than on the click.
//
// The wait is invisible in the ordinary case: the window coming forward takes
// focus from the popover, and the popover hides on focus loss before this ever
// answers.
const goTo = async (sessionId: string): Promise<void> => {
  if (await jump(sessionId)) {
    await close()
  }
}

const navigate = (sessionId: string): void => {
  void goTo(sessionId)
}
</script>

<template>
  <div
    class="flex h-screen w-popover flex-col overflow-hidden rounded-large border border-neutral-4 bg-neutral-1"
  >
    <!--
      Whenever tracking is not installed — not only on the very first launch —
      the app asks for the extension rather than showing a list nothing fills.
      Until tracking has been read no screen is shown, because guessing would
      flash the wrong one.
    -->
    <InstallExtension
      v-if="tracking !== null && tracking.state !== 'installed'"
      class="min-h-0 flex-1"
      :state="tracking.state"
    />

    <SettingsPane
      v-else-if="tracking?.state === 'installed' && showingSettings"
      class="min-h-0 flex-1"
      :settings="settings"
      :failure="settingsFailure"
      @close="showingSettings = false"
      @update="changeSettings"
    />

    <template v-else-if="tracking?.state === 'installed'">
      <PopoverHeader
        :session-count="sessions.sessionCount"
        :unread-count="sessions.unreadCount"
        @settings="openSettings"
      />
      <JumpFailureNotice
        v-if="jumpFailure !== null"
        :failure="jumpFailure"
        @dismiss="dismissJump"
      />
      <SessionList
        v-if="groups.length > 0"
        class="min-h-0 flex-1 overflow-y-auto"
        :groups="groups"
        @read="read"
        @navigate="navigate"
      />
      <EmptyState v-else class="min-h-0 flex-1" />
      <ListFooter @quit="quit" />
    </template>
  </div>
</template>
