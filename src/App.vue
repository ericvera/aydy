<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import JumpFailureNotice from './components/molecules/JumpFailureNotice.vue'
import ListFooter from './components/molecules/ListFooter.vue'
import PopoverHeader from './components/molecules/PopoverHeader.vue'
import PortMoveNotice from './components/molecules/PortMoveNotice.vue'
import EmptyState from './components/organisms/EmptyState.vue'
import ExtensionPrompt from './components/organisms/ExtensionPrompt.vue'
import FirstRun from './components/organisms/FirstRun.vue'
import SessionList from './components/organisms/SessionList.vue'
import SettingsPane from './components/organisms/SettingsPane.vue'
import SettingsUnreadable from './components/organisms/SettingsUnreadable.vue'
import StaleListNotice from './components/organisms/StaleListNotice.vue'
import { useExtension } from './composables/useExtension'
import { useHookStatus } from './composables/useHookStatus'
import { useJump } from './composables/useJump'
import { usePopover } from './composables/usePopover'
import { useSessions } from './composables/useSessions'
import { useSettings } from './composables/useSettings'
import type { SessionGroup } from './types/SessionGroup'
import type { SettingsChange } from './types/SettingsChange'

const { sessions, subscribe, markRead } = useSessions()
const { close, quit } = usePopover()
const {
  status,
  preview,
  failure,
  busy,
  refresh,
  plan,
  install,
  reinstall,
  uninstall,
  revealSettingsFile,
} = useHookStatus()
const {
  settings,
  failure: settingsFailure,
  read: readSettings,
  update,
} = useSettings()
const {
  connected: extensionConnected,
  read: readExtension,
  dismissed: promptDismissed,
  dismiss: dismissPrompt,
} = useExtension()
const { failure: jumpFailure, jump, dismiss: dismissJump } = useJump()

// Settings is a screen inside the popover rather than a window, and it is
// reachable only from the list: the screens that ask for the hooks carry a
// header of their own but no gear in it, so there is nothing to reach settings
// from until they exist.
const showingSettings = ref(false)

onMounted(() => {
  void subscribe()
  void refresh()
})

// The settings are read when the pane opens rather than at startup, so the pane
// shows what the store holds at the moment the user asked to see it.
//
// The hook status is read again for the same reason: the popover's DOM is
// never unmounted — it hides on focus loss — so on a process that has been up
// for days the status is whatever was read at launch. The listener dying, or
// its port moving, would otherwise leave this pane saying events are arriving
// while the list next to it carries the stale notice.
//
// The extension's connection is read here too, and for the plainest reason of
// all: VS Code windows open and close all day, a window closing announces
// nothing, and the only moment the row has to be right is the moment someone is
// looking at it.
const openSettings = (): void => {
  showingSettings.value = true
  void readSettings()
  void refresh()
  void readExtension()
}

const changeSettings = (change: SettingsChange): void => {
  void update(change)
}

const reinstallHooks = (): void => {
  void reinstall()
}

// Removing the hooks does not quit: the app goes back to asking for them, which
// is the screen the routing below already puts up whenever they are absent.
// Leaving settings open would leave that screen behind a pane that no longer
// has hooks to manage.
//
// It closes only once the file says the hooks are gone. `uninstall()` records
// why it failed rather than rejecting, so closing regardless would give a
// failed removal the same disappearing pane a successful one gives — and take
// away the banner saying the settings file was left exactly as it was.
const removeHooks = async (): Promise<void> => {
  await uninstall()

  if (status.value?.installed === false) {
    showingSettings.value = false
  }
}

const groups = computed((): SessionGroup[] => sessions.value.groups)

// Offered only while nothing is reporting, and never again once it has been
// waved off. It is kept off the stale list as well: that state is the app
// saying it cannot see what is running, which is no moment to suggest an
// improvement to how well it works.
const offeringExtension = computed(
  (): boolean =>
    !sessions.value.extensionConnected &&
    !promptDismissed.value &&
    sessions.value.stale === null,
)

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
      Whenever the hooks are absent — not only on the very first launch — the
      app asks for them rather than showing a list it cannot fill. Until the
      settings file has been read neither screen is shown, because guessing
      would flash the wrong one.
    -->
    <FirstRun
      v-if="status !== null && !status.installed"
      class="min-h-0 flex-1"
      :settings-path="status.settingsPath"
      :preview="preview"
      :failure="failure"
      :busy="busy"
      @install="install"
      @plan="plan"
      @reveal="revealSettingsFile"
    />

    <!--
      Below the first-run branch on purpose: hooks removed from the pane put
      that screen up in front of it, which is what returning to the first-run
      state means.
    -->
    <SettingsPane
      v-else-if="status !== null && showingSettings"
      class="min-h-0 flex-1"
      :settings="settings"
      :failure="settingsFailure"
      :hook-status="status"
      :hook-failure="failure"
      :hook-busy="busy"
      :extension-connected="extensionConnected"
      @close="showingSettings = false"
      @update="changeSettings"
      @reinstall="reinstallHooks"
      @remove-hooks="removeHooks"
    />

    <template v-else-if="status !== null">
      <PopoverHeader
        :session-count="sessions.sessionCount"
        :unread-count="sessions.unreadCount"
        :stale="sessions.stale !== null"
        @settings="openSettings"
      />
      <PortMoveNotice
        v-if="sessions.portMove !== null"
        :move="sessions.portMove"
      />
      <JumpFailureNotice
        v-if="jumpFailure !== null"
        :failure="jumpFailure"
        @dismiss="dismissJump"
      />
      <StaleListNotice
        v-if="sessions.stale !== null"
        class="min-h-0 flex-1 overflow-y-auto"
        :session-count="sessions.stale.sessionCount"
        :last-seen-ago-ms="sessions.stale.lastSeenAgoMs"
      />
      <SessionList
        v-else-if="groups.length > 0"
        class="min-h-0 flex-1 overflow-y-auto"
        :groups="groups"
        @read="read"
        @navigate="navigate"
      />
      <EmptyState v-else class="min-h-0 flex-1" />
      <ExtensionPrompt v-if="offeringExtension" @dismiss="dismissPrompt" />
      <ListFooter @quit="quit" />
    </template>

    <!--
      The status is unknown and the read that would have settled it failed, so
      neither screen above can be shown honestly. The failure is shown instead:
      recording one and rendering nothing leaves an empty popover that never
      reads the file again.
    -->
    <SettingsUnreadable
      v-else-if="failure !== null"
      class="min-h-0 flex-1"
      :failure="failure"
      :busy="busy"
      @retry="plan"
      @reveal="revealSettingsFile"
    />
  </div>
</template>
