import { aSession, renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { SessionGroup } from '../../types/SessionGroup'
import SessionList from './SessionList.vue'

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

const okvenAttribution: SessionGroup = {
  kind: 'project',
  repo: 'okven',
  branch: 'eric/attribution-2',
  sessions: [
    aSession({
      id: 'b',
      label: 'Review attribution mock data test file',
      status: 'needsInput',
      unread: true,
    }),
    aSession({
      id: 'c',
      label: 'Fix permission denied for code command',
      status: 'working',
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
      status: 'working',
      jumpable: false,
    }),
  ],
}

const list = (groups: SessionGroup[]): VueWrapper =>
  mount(SessionList, { props: { groups } })

const rowAt = (wrapper: VueWrapper, index: number): string => {
  const element = wrapper.findAll('button')[index]?.element

  return element === undefined ? '' : renderedText(element)
}

const reordered = (group: SessionGroup): SessionGroup => ({
  ...group,
  sessions: [...group.sessions].reverse(),
})

/**
 * Grouping and ordering are done in Rust so the list and the menu bar icon
 * cannot disagree. The list renders the snapshot in the order it arrives and
 * sorts nothing of its own.
 */
it('renders the groups in the order the snapshot supplies', () => {
  const rendered = renderedText(
    list([notJumpable, okvenMain, okvenAttribution]).element,
  )

  expect(rendered).toBe(
    'Not jumpable Review yarn.lock dependencies okven · main Review and prioritize customer requests okven · eric/attribution-2 Review attribution mock data test file Fix permission denied for code command',
  )
})

it('renders the rows in the order their group supplies', () => {
  expect(renderedText(list([okvenAttribution]).element)).toBe(
    'okven · eric/attribution-2 Review attribution mock data test file Fix permission denied for code command',
  )
})

it('draws the whole list', () => {
  expect(list([okvenMain, notJumpable]).html()).toMatchInlineSnapshot(`
    "<div class="group/list">
      <section class="group-divider">
        <p class="px-5 pb-2 pt-4 text-0 font-3 uppercase text-neutral-6">okven · main</p><button type="button" class="group/row row-divider flex w-full flex-col gap-1 px-5 py-2 text-left cursor-pointer hover:bg-neutral-2"><span class="flex w-full items-center gap-3"><span class="h-3 w-3 shrink-0 rounded-full bg-status-done"></span><span class="truncate text-2 title-unread min-w-0 flex-1">Review and prioritize customer requests</span>
          <!--v-if--></span><span class="hidden invisible flex-wrap gap-3 text-0 text-neutral-5 group-hover/list:flex group-hover/row:visible"><span>Done</span><span>Working time 0s</span><span>Longest turn 0s</span></span>
        </button>
      </section>
      <section class="group-divider">
        <p class="px-5 pb-2 pt-4 text-0 font-3 uppercase text-neutral-6">Not jumpable</p><button type="button" class="group/row row-divider flex w-full flex-col gap-1 px-5 py-2 text-left cursor-default"><span class="flex w-full items-center gap-3"><span class="h-3 w-3 shrink-0 rounded-full bg-status-working"></span><span class="truncate text-2 title-read min-w-0 flex-1">Review yarn.lock dependencies</span>
          <!--v-if--></span><span class="hidden invisible flex-wrap gap-3 text-0 text-neutral-5 group-hover/list:flex group-hover/row:visible"><span>Working</span><span>Working time 0s</span><span>Longest turn 0s</span></span>
        </button>
      </section>
    </div>"
  `)
})

/**
 * Rows are keyed by session id, so a snapshot that changes a title patches the
 * row in place. Keyed by position instead, a group arriving above another
 * would remount every row below it — titles flicker and the hovered row jumps.
 */
it('keeps the same row element when a title changes', async () => {
  const wrapper = list([okvenMain])
  const before = wrapper.get('button').element

  await wrapper.setProps({
    groups: [
      {
        ...okvenMain,
        sessions: [
          aSession({ id: 'a', label: 'A title Claude has thought better of' }),
        ],
      },
    ],
  })

  expect(wrapper.get('button').element).toBe(before)
  expect(renderedText(wrapper.element)).toContain(
    'A title Claude has thought better of',
  )
})

/**
 * A row's element belongs to its session, not to its slot. Sessions reorder
 * inside a group whenever one changes status or becomes the most recent, which
 * happens constantly, so keying by position would have Vue rewrite each row's
 * contents in place instead of moving the row.
 */
it('keeps each row with its own session when the group reorders', async () => {
  const wrapper = list([okvenAttribution])
  const before = wrapper.findAll('button')[0]?.element

  await wrapper.setProps({ groups: [reordered(okvenAttribution)] })

  expect(wrapper.findAll('button')[1]?.element).toBe(before)
  expect(rowAt(wrapper, 1)).toBe('Review attribution mock data test file')
})

/**
 * The same rule seen from where the user feels it. Sessions reorder under a
 * still pointer on every status change, up to ten times a second, and the
 * detail a row reveals must belong to whichever row the pointer is actually
 * over afterwards. That is why the reveal is `:hover` and not a flag on the
 * row component: a flag survives the reorder, and the row that carries it away
 * from the pointer goes on showing its working time, longest turn and status
 * with nothing near it.
 */
it('leaves no row showing its detail after the group reorders under the pointer', async () => {
  const wrapper = list([okvenAttribution])

  await wrapper.findAll('button')[0]?.trigger('mouseenter')
  await wrapper.setProps({ groups: [reordered(okvenAttribution)] })

  expect(rowAt(wrapper, 0)).toBe('Fix permission denied for code command')
  expect(rowAt(wrapper, 1)).toBe('Review attribution mock data test file')
})

/**
 * Groups are keyed by their identity rather than their position, so a group
 * that becomes the most recently active and moves to the top does not remount
 * the rows of the groups it overtook.
 */
it('keeps the same row element when a group moves up the list', async () => {
  const wrapper = list([okvenMain, notJumpable])
  const before = wrapper.get('button').element

  await wrapper.setProps({ groups: [notJumpable, okvenMain] })

  const [first] = wrapper.findAll('button')

  expect(first?.element).not.toBe(before)
  expect(wrapper.findAll('button')[1]?.element).toBe(before)
})

it('passes a click through as a request to mark the session read', async () => {
  const wrapper = list([okvenAttribution])

  await wrapper.findAll('button')[1]?.trigger('click')

  expect(wrapper.emitted('read')).toStrictEqual([['c']])
  expect(wrapper.emitted('navigate')).toStrictEqual([['c']])
})

it('passes a click on a row that cannot be jumped to through as read alone', async () => {
  const wrapper = list([notJumpable])

  await wrapper.get('button').trigger('click')

  expect(wrapper.emitted('read')).toStrictEqual([['d']])
  expect(wrapper.emitted('navigate')).toBeUndefined()
})

/**
 * At most one row says the user is in it. The registry guarantees that by
 * holding one focused window, and the list must not spread the mark across the
 * rows it renders.
 */
it('marks at most one row as the session the user is in', () => {
  const current = aSession({
    id: 'e',
    label: 'The one you are in',
    isCurrent: true,
  })
  const rendered = list([
    okvenMain,
    { ...okvenAttribution, sessions: [...okvenAttribution.sessions, current] },
  ]).text()

  expect(rendered.match(/you are here/g)).toStrictEqual(['you are here'])
})

it('renders nothing at all for a snapshot with no groups', () => {
  expect(list([]).text()).toBe('')
})

/**
 * The list is the outer half of the row hover reveal. Every row's detail line
 * takes its space as soon as the pointer is on the list, so revealing one row's
 * detail changes no row's height and nothing under the pointer moves; reserving
 * it per row instead made the whole list below the pointer jump on every row
 * boundary it crossed.
 */
it('marks itself as the group the rows reserve their detail space from', () => {
  expect(list([okvenMain]).classes()).toContain('group/list')
})
