import { aSession, renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { afterEach, expect, it, vi } from 'vitest'

import type { JumpOutcome } from './types/JumpOutcome'
import type { SessionGroup } from './types/SessionGroup'
import type { SessionList } from './types/SessionList'
import type { Settings } from './types/Settings'
import type { TrackingStatus } from './types/TrackingStatus'

interface FakeChannel {
  onmessage: (list: SessionList) => void
}

const TRACKING: TrackingStatus = { state: 'installed' }

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
  tracking: TrackingStatus
  settings: Settings
  jump: JumpOutcome
}

const { channels, invoked, answers } = vi.hoisted(
  (): { channels: FakeChannel[]; invoked: Invocation[]; answers: Answers } => ({
    channels: [],
    invoked: [],
    answers: {
      tracking: { state: 'installed' },
      settings: {
        finishedSound: true,
        needsInputSound: true,
        launchAtLogin: true,
      },
      jump: 'jumped',
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

    if (command === 'tracking_status') {
      return answers.tracking
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
): SessionList => ({
  groups,
  sessionCount,
  unreadCount,
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

const mounted: VueWrapper[] = []

// Unmounting stops the tracking poll, which would otherwise fire into a later
// test's invocations.
afterEach(() => {
  for (const wrapper of mounted.splice(0)) {
    wrapper.unmount()
  }
})

/**
 * Mounts the popover and lets it subscribe and read the tracking status.
 */
const popover = async (
  tracking: TrackingStatus = TRACKING,
): Promise<VueWrapper> => {
  channels.length = 0
  invoked.length = 0
  answers.tracking = tracking
  answers.settings = SETTINGS
  answers.jump = 'jumped'
  vi.resetModules()

  const App = (await import('./App.vue')).default
  const wrapper = mount(App)
  mounted.push(wrapper)
  await flushPromises()

  return wrapper
}

/**
 * Mounts the popover, lets it subscribe, and pushes one snapshot into it.
 */
const showing = async (snapshot: SessionList): Promise<VueWrapper> => {
  const wrapper = await popover()

  channels[0]?.onmessage(snapshot)
  await nextTick()

  return wrapper
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
    { command: 'tracking_status', args: undefined },
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
    { command: 'tracking_status', args: undefined },
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
    'tracking_status',
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
 * Quit lives in the list footer and only there — no other screen carries one.
 */
it('quits from the footer', async () => {
  const wrapper = await showing(list(0, 0))

  await wrapper.get('footer button').trigger('click')
  await flushPromises()

  expect(invoked.map((call) => call.command)).toStrictEqual([
    'subscribe_sessions',
    'tracking_status',
    'quit',
  ])
})

/**
 * Whenever tracking is not installed — not only on the very first launch — the
 * app asks for the extension, because a list nothing fills is not an honest
 * empty list.
 */
it('shows the install screen and no list when tracking is not installed', async () => {
  const wrapper = await popover({ state: 'notInstalled' })

  expect(renderedText(wrapper.element)).toContain(
    'Install the AYDY extension in VS Code',
  )
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'EmptyState' }).exists()).toBe(false)
})

it('says the hooks are missing when the extension is there without them', async () => {
  const wrapper = await popover({ state: 'hooksMissing' })

  expect(renderedText(wrapper.element)).toContain("AYDY's hooks are missing")
  expect(wrapper.findComponent({ name: 'EmptyState' }).exists()).toBe(false)
})

it('shows the session list and not the install screen once tracking is installed', async () => {
  const wrapper = await showing(list(1, 0, [okvenMain]))

  expect(wrapper.findComponent({ name: 'InstallExtension' }).exists()).toBe(
    false,
  )
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(true)
})

/**
 * Installing the extension is the only thing that can be done until tracking
 * exists: the install screen offers no settings, and no quit either.
 */
it('offers nothing but the install on the install screen', async () => {
  const wrapper = await popover({ state: 'notInstalled' })

  expect(wrapper.find('footer').exists()).toBe(false)
  expect(renderedText(wrapper.element)).not.toContain('Quit')
  expect(wrapper.find('[aria-label="Settings"]').exists()).toBe(false)
})

/** The screen clears by itself once the extension's files appear. */
it('moves from the install screen to the list once tracking appears', async () => {
  vi.useFakeTimers()

  try {
    const wrapper = await popover({ state: 'notInstalled' })

    answers.tracking = { state: 'installed' }
    await vi.advanceTimersByTimeAsync(2000)

    expect(wrapper.findComponent({ name: 'InstallExtension' }).exists()).toBe(
      false,
    )
    expect(wrapper.findComponent({ name: 'EmptyState' }).exists()).toBe(true)
  } finally {
    vi.useRealTimers()
  }
})

/**
 * Polling stops once tracking is installed and the popover is never unmounted,
 * so opening it is what notices the extension was uninstalled since.
 */
it('brings the install screen back when the popover regains focus without tracking', async () => {
  const wrapper = await showing(list(1, 0, [okvenMain]))

  answers.tracking = { state: 'notInstalled' }
  window.dispatchEvent(new Event('focus'))
  await flushPromises()

  expect(wrapper.findComponent({ name: 'InstallExtension' }).exists()).toBe(
    true,
  )
  expect(wrapper.findComponent({ name: 'SessionList' }).exists()).toBe(false)
})

/**
 * Neither screen is right until tracking has been read, and flashing the wrong
 * one is worse than showing nothing for a frame.
 */
it('shows neither screen until tracking has been read', async () => {
  channels.length = 0
  invoked.length = 0
  answers.tracking = TRACKING
  vi.resetModules()

  const App = (await import('./App.vue')).default
  const wrapper = mount(App)
  mounted.push(wrapper)

  expect(renderedText(wrapper.element)).toBe('')

  await flushPromises()

  expect(renderedText(wrapper.element)).not.toBe('')
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
 * The install screen has a header — the same shared one every screen has — but
 * no gear in it, so there is nothing to reach settings from.
 */
it('offers no way into settings before tracking is installed', async () => {
  const wrapper = await popover({ state: 'hooksMissing' })

  expect(wrapper.find('header').exists()).toBe(true)
  expect(wrapper.find('[aria-label="Settings"]').exists()).toBe(false)
  expect(wrapper.findComponent({ name: 'SettingsPane' }).exists()).toBe(false)
})

/**
 * Opening settings is a moment someone is looking, so tracking is asked again
 * there as well.
 */
it('reads tracking again when settings opens', async () => {
  const wrapper = await showing(list(0, 0))

  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  expect(
    invoked.filter((call) => call.command === 'tracking_status'),
  ).toHaveLength(2)
})

it('reports session tracking as installed in settings', async () => {
  const wrapper = await showing(list(0, 0))

  await wrapper.get('[aria-label="Settings"]').trigger('click')
  await flushPromises()

  expect(renderedText(wrapper.element)).toContain('Session tracking Installed')
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
