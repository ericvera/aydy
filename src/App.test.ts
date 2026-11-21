import { aSession, renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { expect, it, vi } from 'vitest'

import type { ExtensionStatus } from './types/ExtensionStatus'
import type { HookPreview } from './types/HookPreview'
import type { JumpOutcome } from './types/JumpOutcome'
import type { HookStatus } from './types/HookStatus'
import type { PortMove } from './types/PortMove'
import type { SessionGroup } from './types/SessionGroup'
import type { SessionList } from './types/SessionList'
import type { Settings } from './types/Settings'
import type { StaleList } from './types/StaleList'

interface FakeChannel {
  onmessage: (list: SessionList) => void
}

const INSTALLED: HookStatus = {
  installed: true,
  hookCount: 5,
  settingsPath: '~/.claude/settings.json',
  receivingEvents: true,
}

const ABSENT: HookStatus = {
  installed: false,
  hookCount: 0,
  settingsPath: '~/.claude/settings.json',
  receivingEvents: false,
}

const PREVIEW: HookPreview = {
  hookCount: 5,
  added: [{ event: 'Stop', kind: 'http', timeoutSeconds: 2 }],
  removed: [],
  hooksJson: '{}',
}

const SETTINGS: Settings = {
  finishedSound: true,
  needsInputSound: true,
  launchAtLogin: true,
}

interface Invocation {
  command: string
  args: Record<string, unknown> | undefined
}

interface Answers {
  hooks: HookStatus
  settings: Settings
  extension: ExtensionStatus
  jump: JumpOutcome
  fails: Record<string, unknown>
}

const { channels, invoked, answers } = vi.hoisted(
  (): { channels: FakeChannel[]; invoked: Invocation[]; answers: Answers } => ({
    channels: [],
    invoked: [],
    answers: {
      hooks: {
        installed: true,
        hookCount: 5,
        settingsPath: '',
        receivingEvents: true,
      },
      settings: {
        finishedSound: true,
        needsInputSound: true,
        launchAtLogin: true,
      },
      extension: { connected: false, windowCount: 0 },
      jump: 'jumped',
      fails: {},
    },
  }),
)

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: (list: SessionList) => void = () => {}

    constructor() {
      channels.push(this)
    }
  },
  invoke: async (
    command: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> => {
    invoked.push({ command, args })

    if (command in answers.fails) {
      throw answers.fails[command]
    }

    if (command === 'hook_status') {
      return answers.hooks
    }

    if (command === 'preview_hook_install') {
      return PREVIEW
    }

    if (command === 'extension_status') {
      return answers.extension
    }

    if (command === 'jump_to_session') {
      return answers.jump
    }

    return command.endsWith('_settings') ? answers.settings : undefined
  },
}))

const list = (
  sessionCount: number,
  unreadCount: number,
  groups: SessionGroup[] = [],
  health: {
    portMove?: PortMove
    stale?: StaleList
    extensionConnected?: boolean
  } = {},
): SessionList => ({
  groups,
  sessionCount,
  unreadCount,
  portMove: health.portMove ?? null,
  stale: health.stale ?? null,
  // Connected unless a test is about the prompt, so that the offer of the
  // extension only shows up in the assertions that are looking for it.
  extensionConnected: health.extensionConnected ?? true,
})

const okvenMain: SessionGroup = {
  kind: 'project',
  repo: 'okven',
  branch: 'main',
  sessions: [
    aSession({
      id: 'a',
      label: 'Review and prioritize customer requests',
      unread: true,
    }),
  ],
}

const notJumpable: SessionGroup = {
  kind: 'notJumpable',
  repo: null,
  branch: null,
  sessions: [
    aSession({
      id: 'd',
      label: 'Review yarn.lock dependencies',
      jumpable: false,
    }),
  ],
}

/**
 * Mounts the popover and lets it subscribe and read the hook status.
 */
const popover = async (
  hooks: HookStatus = INSTALLED,
  fails: Record<string, unknown> = {},
): Promise<VueWrapper> => {
  channels.length = 0
  invoked.length = 0
  answers.hooks = hooks
  answers.settings = SETTINGS
  answers.extension = { connected: false, windowCount: 0 }
  answers.jump = 'jumped'
  answers.fails = fails
  window.localStorage.clear()
  vi.resetModules()

  const App = (await import('./App.vue')).default
  const wrapper = mount(App)
  await flushPromises()

  return wrapper
}

/**
 * Mounts the popover, lets it subscribe, and pushes one snapshot into it.
 */
const showing = async (snapshot: SessionList): Promise<VueWrapper> => {
  const wrapper = await popover(INSTALLED)

  channels[0]?.onmessage(snapshot)
  await nextTick()

  return wrapper
}

const pressLabelled = async (
  wrapper: VueWrapper,
  label: string,
): Promise<void> => {
  const button = wrapper
    .findAll('button')
    .find((candidate) => candidate.text() === label)

  if (button === undefined) {
    throw new Error(`there is no button labelled ${label}`)
  }

  await button.trigger('click')
}

const text = async (snapshot: SessionList): Promise<string> =>
  renderedText((await showing(snapshot)).element)

it('shows the empty state when nothing is running', async () => {
  expect(await text(list(0, 0))).toBe(
    'No sessions ⚙ Nothing running Start Claude Code in any project and it shows up here automatically. Quit',
  )
})

it('shows the counted header and the grouped list when sessions are running', async () => {
  expect(await text(list(1, 1, [okvenMain]))).toBe(
    '1 session · 1 new ⚙ okven · main Review and prioritize customer requests Quit',
  )
})

it('drops the empty state as soon as there is a group to show', async () => {
  const rendered = await text(list(1, 0, [okvenMain]))

  expect(rendered).not.toContain('Nothing running')
})

/**
 * The window is transparent and undecorated, so the shell is the only thing
 * drawing the popover's corner, hairline and background — the mock's
 * `.popover`. Nothing below it can put them back if it loses them.
 *
 * It is also a column, which is what lets the list scroll between a fixed
 * header and a fixed footer rather than growing the window.
 */
it('draws the popover shell as a column', async () => {
  const wrapper = await popover()

  expect(wrapper.classes().sort()).toMatchInlineSnapshot(`
    [
      "bg-neutral-1",
      "border",
      "border-neutral-4",
      "flex",
      "flex-col",
      "h-screen",
      "overflow-hidden",
      "rounded-large",
      "w-popover",
    ]
  `)
})

/**
 * The popover is a fixed height, so a list longer than it has to scroll inside
 * the shell rather than push the header or the footer out of the window.
 *
 * The list's own `group/list` sits alongside them, which is also the proof that
 * these three arrive at all: they are fallthrough attributes, and a list that
 * stopped having a single root element would silently drop every one of them
 * and scroll the whole popover instead.
 */
it('scrolls the list and leaves the header and the footer fixed', async () => {
  const wrapper = await showing(list(1, 0, [okvenMain]))

  expect(
    wrapper.getComponent({ name: 'SessionList' }).classes().sort(),
  ).toStrictEqual(['flex-1', 'group/list', 'min-h-0', 'overflow-y-auto'])
  expect(wrapper.get('header').classes()).toContain('shrink-0')
  expect(wrapper.get('footer').classes()).toContain('shrink-0')
})

it('marks a session read when its row is clicked', async () => {
  const wrapper = await showing(list(1, 1, [okvenMain]))

  await wrapper
    .getComponent({ name: 'SessionList' })
    .get('button')
    .trigger('click')
  await flushPromises()

  expect(invoked).toStrictEqual([
    { command: 'subscribe_sessions', args: { channel: channels[0] } },
    { command: 'hook_status', args: undefined },
    { command: 'mark_read', args: { sessionId: 'a' } },
    { command: 'jump_to_session', args: { sessionId: 'a' } },
    { command: 'close_popover', args: undefined },
  ])
})

/**
 * A click that navigates closes the popover; a click that only marks a session
 * read leaves it open, since the user is still reading the list.
 */
it('leaves the popover open when a row that cannot be jumped to is clicked', async () => {
  const wrapper = await showing(list(1, 1, [notJumpable]))

  await wrapper
    .getComponent({ name: 'SessionList' })
    .get('button')
    .trigger('click')
  await flushPromises()

  expect(invoked).toStrictEqual([
    { command: 'subscribe_sessions', args: { channel: channels[0] } },
    { command: 'hook_status', args: undefined },
    { command: 'mark_read', args: { sessionId: 'd' } },
  ])
})

/**
 * The click went nowhere, so the popover stays where it is — the same thing a
 * row that cannot be jumped to has always done, and the same thing a session
 * no window can be found for is owed. It is not an error.
 */
it('leaves the popover open and says nothing when a jump found no window', async () => {
  const wrapper = await showing(list(1, 1, [okvenMain]))
  answers.jump = 'noWindow'

  await wrapper
    .getComponent({ name: 'SessionList' })
    .get('button')
    .trigger('click')
  await flushPromises()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'subscribe_sessions',
    'hook_status',
    'mark_read',
    'jump_to_session',
  ])
  expect(renderedText(wrapper.element)).not.toContain(
    'No VS Code window took the jump',
  )
})

/**
 * A jump no window answered has to be said out loud rather than reported as a
 * success the user can see did not happen.
 */
it('says so when no window answered the jump', async () => {
  const wrapper = await showing(list(1, 1, [okvenMain]))
  answers.jump = 'unanswered'

  await wrapper
    .getComponent({ name: 'SessionList' })
    .get('button')
    .trigger('click')
  await flushPromises()

  expect(renderedText(wrapper.element)).toContain(
    'No VS Code window took the jump. The window that had this session may be closed.',
  )
  expect(invoked.map((call) => call.command)).not.toContain('close_popover')
})

/**
 * The most damaging thing this app can do, and the reason the reveal is
 * guarded on both sides. If it happens anyway the user is told, because a
 * second live session they do not know about is worse than one they do.
 */
it('warns when a reveal opened a tab instead of revealing one', async () => {
  const wrapper = await showing(list(1, 1, [okvenMain]))
  answers.jump = 'duplicated'

  await wrapper
    .getComponent({ name: 'SessionList' })
    .get('button')
    .trigger('click')
  await flushPromises()

  expect(renderedText(wrapper.element)).toContain('A new Claude tab opened.')
})

it('takes the jump notice away when it is dismissed', async () => {
  const wrapper = await showing(list(1, 1, [okvenMain]))
  answers.jump = 'unanswered'
  await wrapper
    .getComponent({ name: 'SessionList' })
    .get('button')
    .trigger('click')
  await flushPromises()

  await wrapper.get('[aria-label="Dismiss"]').trigger('click')

  expect(renderedText(wrapper.element)).not.toContain(
    'No VS Code window took the jump',
  )
})

/**
 * A companion nobody hears about does not get installed, and several
 * behaviours degrade quietly without it.
 */
it('offers the companion extension while no window is reporting', async () => {
  const wrapper = await showing(
    list(1, 0, [okvenMain], { extensionConnected: false }),
  )

  expect(renderedText(wrapper.element)).toContain('Add the VS Code extension')
})

/**
 * Windows register seconds after the editor starts, which is why this rides on
 * the snapshot: an answer taken once when the popover was built would offer
 * the extension to people who already have it.
 */
it('stops offering the extension once a window reports', async () => {
  const wrapper = await showing(
    list(1, 0, [okvenMain], { extensionConnected: false }),
  )
  expect(renderedText(wrapper.element)).toContain('Add the VS Code extension')

  channels[0]?.onmessage(list(1, 0, [okvenMain], { extensionConnected: true }))
  await nextTick()

  expect(renderedText(wrapper.element)).not.toContain(
    'Add the VS Code extension',
  )
})

/** Offered once. A prompt that came back every launch would be a nag. */
it('never offers the extension again once it has been waved off', async () => {
  const wrapper = await showing(
    list(1, 0, [okvenMain], { extensionConnected: false }),
  )

  await wrapper.get('[aria-label="Dismiss"]').trigger('click')
  await nextTick()

  expect(renderedText(wrapper.element)).not.toContain(
    'Add the VS Code extension',
  )
  expect(window.localStorage.getItem('aydy.extension-prompt-dismissed')).toBe(
    'yes',
  )
})

/**
 * The stale list is the app saying it cannot see what is running, which is no
 * moment to suggest an improvement to how well it works.
 */
it('does not offer the extension over a stale list', async () => {
  const wrapper = await showing(
    list(0, 0, [], {
      extensionConnected: false,
      stale: { sessionCount: 3, lastSeenAgoMs: 360_000 },
    }),
  )

  expect(renderedText(wrapper.element)).not.toContain(
    'Add the VS Code extension',
  )
})

/**
 * Quit lives in the list footer and only there — no other screen carries one.
 */
it('quits from the footer', async () => {
  const wrapper = await showing(list(0, 0))

  await wrapper.get('footer button').trigger('click')
  await flushPromises()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'subscribe_sessions',
    'hook_status',
    'quit',
  ])
})

/**
 * Whenever the hooks are absent — not only on the very first launch — the app
 * asks for them, because a list it cannot fill is not an honest empty list.
 */
it('shows the first-run screen when the hooks are absent', async () => {
  const wrapper = await popover(ABSENT)

  expect(renderedText(wrapper.element)).toContain(
    'Track your Claude Code sessions',
  )
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'EmptyState' }).exists()).toBe(false)
})

it('shows the session list and not the first-run screen once the hooks exist', async () => {
  const wrapper = await showing(list(1, 0, [okvenMain]))

  expect(wrapper.findComponent({ name: 'FirstRun' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(true)
})

/**
 * Installing the hooks is the only action available until they exist: the
 * first-run screen offers no settings, and no quit either.
 */
it('offers nothing but the install on the first-run screen', async () => {
  const wrapper = await popover(ABSENT)

  expect(wrapper.find('footer').exists()).toBe(false)
  expect(renderedText(wrapper.element)).not.toContain('Quit')
  expect(renderedText(wrapper.element)).not.toContain('Settings')
})

/**
 * Neither screen is right until the settings file has been read, and flashing
 * the wrong one is worse than showing nothing for a frame.
 */
it('shows neither screen until the settings file has been read', async () => {
  channels.length = 0
  invoked.length = 0
  answers.hooks = INSTALLED
  answers.fails = {}
  vi.resetModules()

  const App = (await import('./App.vue')).default
  const wrapper = mount(App)

  expect(renderedText(wrapper.element)).toBe('')

  await flushPromises()

  expect(renderedText(wrapper.element)).not.toBe('')
})

/**
 * The move is already made and the hooks already rewritten, so the notice sits
 * above whatever the list actually contains rather than replacing it.
 */
it('shows the port move above the list', async () => {
  const wrapper = await showing(
    list(1, 0, [okvenMain], { portMove: { from: 47_823, to: 47_824 } }),
  )

  expect(renderedText(wrapper.element)).toContain(
    'Port 47823 was taken, so AYDY moved to 47824 and updated its hooks.',
  )
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(true)
})

it('shows no port move notice when the listener never moved', async () => {
  const wrapper = await showing(list(1, 0, [okvenMain]))

  expect(wrapper.findComponent({ name: 'PortMoveNotice' }).exists()).toBe(false)
})

/**
 * A list that is not being updated must not be presented as current, and an
 * empty one would read as "nothing is running".
 */
it('replaces the list with the stale notice while events are going nowhere', async () => {
  const wrapper = await showing(
    list(0, 0, [], { stale: { sessionCount: 3, lastSeenAgoMs: 360_000 } }),
  )

  expect(renderedText(wrapper.element)).toContain(
    'Last seen 3 sessions, 6m ago',
  )
  expect(wrapper.findComponent({ name: 'EmptyState' }).exists()).toBe(false)
})

/**
 * The header carries the live count and the stale notice carries the frozen
 * one, so a header left counting contradicts the notice under it: the
 * reconciler goes on reaping sessions on its own scan while events cannot
 * reach the app, and the live count drains to zero during the spell.
 */
it('heads the stale list with the state rather than a count that has drained away', async () => {
  const wrapper = await showing(
    list(0, 0, [], { stale: { sessionCount: 3, lastSeenAgoMs: 360_000 } }),
  )

  const rendered = renderedText(wrapper.element)

  expect(rendered).toContain('Reconnecting')
  expect(rendered).not.toContain('No sessions')
})

/**
 * A settings file that could not be read at all leaves the app unable to say
 * whether its hooks are there, so neither screen can be shown — but the failure
 * has to be, or the popover is an empty shell with no way to read it again.
 */
it('shows the failure when the settings file could not be read at all', async () => {
  const wrapper = await popover(ABSENT, {
    hook_status: { reason: 'The file could not be read.' },
  })

  expect(renderedText(wrapper.element)).toContain(
    'AYDY cannot read your settings',
  )
  expect(renderedText(wrapper.element)).toContain('The file could not be read.')
  expect(wrapper.findComponent({ name: 'FirstRun' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(false)
})

it('reads the settings file again when that failure is retried', async () => {
  const wrapper = await popover(ABSENT, {
    hook_status: { reason: 'The file could not be read.' },
  })
  answers.fails = {}

  await wrapper.get('button').trigger('click')
  await flushPromises()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'subscribe_sessions',
    'hook_status',
    'hook_status',
    'preview_hook_install',
  ])
  expect(wrapper.findComponent({ name: 'FirstRun' }).exists()).toBe(true)
})

/**
 * The stale treatment cannot apply before the hooks exist: nothing is being
 * sent then, so nothing is being lost, and a freshly installed app would
 * otherwise present itself as broken.
 */
it('does not call the list stale before the hooks exist', async () => {
  const wrapper = await popover(ABSENT)

  channels[0]?.onmessage(
    list(0, 0, [], { stale: { sessionCount: 3, lastSeenAgoMs: 360_000 } }),
  )
  await nextTick()

  expect(wrapper.findComponent({ name: 'StaleListNotice' }).exists()).toBe(
    false,
  )
  expect(renderedText(wrapper.element)).toContain(
    'Track your Claude Code sessions',
  )
})

/**
 * Settings is a screen inside the popover, reachable from the list and nowhere
 * else, with a way back to it. The settings are read when it opens rather than
 * at startup, so it shows what the store holds at the moment it is asked for.
 */
it('opens settings from the list and goes back to it', async () => {
  const wrapper = await showing(list(1, 0, [okvenMain]))

  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(true)
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(false)
  expect(invoked.map((call) => call.command)).toContain('read_settings')
  expect(renderedText(wrapper.element)).toContain('Launch at login')

  await wrapper.get('[aria-label="Close settings"]').trigger('click')
  await flushPromises()

  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(true)
})

/**
 * Installing the hooks is the only thing that can be done until they exist. The
 * screen has a header — the same shared one every screen has — but no gear in
 * it, so there is nothing to reach settings from.
 */
it('offers no way into settings before the hooks exist', async () => {
  const wrapper = await popover(ABSENT)

  expect(wrapper.find('header').exists()).toBe(true)
  expect(wrapper.find('[aria-label="Settings"]').exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(false)
})

/**
 * Task 4.2 built this row against a permanent placeholder, because nothing
 * reported anything yet. It is read when the pane opens rather than kept up to
 * date: a VS Code window closing announces nothing, so the only moment the row
 * has to be right is the moment someone is looking at it.
 */
it('reports the companion extension as connected once a window has registered', async () => {
  const wrapper = await showing(list(0, 0))
  answers.extension = { connected: true, windowCount: 2 }

  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  expect(invoked.map((call) => call.command)).toContain('extension_status')
  expect(renderedText(wrapper.element)).toContain('VS Code extension Connected')
})

it('reports the companion extension as not connected while no window has', async () => {
  const wrapper = await showing(list(0, 0))

  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  expect(renderedText(wrapper.element)).toContain(
    'VS Code extension Not connected',
  )
})

it('sends a setting that moved in the pane to Rust', async () => {
  const wrapper = await showing(list(0, 0))
  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  await wrapper.get('[aria-label="Launch at login"]').trigger('click')
  await flushPromises()

  expect(invoked.at(-1)).toStrictEqual({
    command: 'update_settings',
    args: { change: { launchAtLogin: false } },
  })
})

/**
 * REQ-RESIL-7. Removing the hooks does not end the app: it goes back to asking
 * for them, which is the screen shown whenever they are absent. Leaving the
 * pane up would leave a screen managing hooks that are no longer there.
 */
it('returns to the first-run screen when the hooks are removed, without quitting', async () => {
  const wrapper = await showing(list(0, 0))
  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  answers.hooks = ABSENT
  await pressLabelled(wrapper, 'Remove hooks')
  await flushPromises()

  expect(invoked.map((call) => call.command)).toContain('uninstall_hooks')
  expect(invoked.map((call) => call.command)).not.toContain('quit')
  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'FirstRun' }).exists()).toBe(true)
})

/**
 * A removal that failed must not read as one that worked. `uninstall()` records
 * the reason rather than rejecting, so a pane that closed regardless would give
 * the failure the same disappearing pane a success gives — while the hooks were
 * still installed and the banner saying the file was left alone never appeared.
 */
it('keeps the pane up, with the reason, when the removal failed', async () => {
  const wrapper = await popover(INSTALLED, {
    uninstall_hooks: { reason: 'The file changed while AYDY was writing it.' },
  })
  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  await pressLabelled(wrapper, 'Remove hooks')
  await flushPromises()

  const rendered = renderedText(wrapper.element)

  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(true)
  expect(wrapper.findComponent({ name: 'FirstRun' }).exists()).toBe(false)
  expect(rendered).toContain('Nothing in ~/.claude/settings.json was modified.')
  expect(rendered).toContain('The file changed while AYDY was writing it.')
})

/**
 * The popover's DOM is never unmounted — it hides on focus loss — so a status
 * read once at launch is what a process up for days would still be showing. The
 * listener dying or its port moving in between would leave the pane reporting
 * that events are arriving while the list beside it carries the stale notice.
 */
it('reads the hook status again each time settings opens', async () => {
  const wrapper = await showing(list(0, 0))

  answers.hooks = { ...INSTALLED, receivingEvents: false }
  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  expect(invoked.filter((call) => call.command === 'hook_status')).toHaveLength(
    2,
  )
  expect(renderedText(wrapper.element)).toContain('Not receiving events')
})

it('repairs the hooks from the pane and reads the file back', async () => {
  const wrapper = await showing(list(0, 0))
  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  await pressLabelled(wrapper, 'Reinstall')
  await flushPromises()

  expect(invoked.map((call) => call.command).slice(-2)).toStrictEqual([
    'reinstall_hooks',
    'hook_status',
  ])
  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(true)
})

/**
 * The pane is left behind as well as hidden. Installing the hooks again from
 * the first-run screen returns to the list, which is where a user who has just
 * set the app up expects to land — not back inside the settings they were in
 * several steps ago.
 */
it('does not come back to settings when the hooks are installed again', async () => {
  const wrapper = await showing(list(0, 0))
  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  answers.hooks = ABSENT
  await pressLabelled(wrapper, 'Remove hooks')
  await flushPromises()

  answers.hooks = INSTALLED
  await pressLabelled(wrapper, 'Install hooks')
  await flushPromises()

  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'FirstRun' }).exists()).toBe(false)
  expect(wrapper.find('[aria-label="Settings"]').exists()).toBe(true)
})
