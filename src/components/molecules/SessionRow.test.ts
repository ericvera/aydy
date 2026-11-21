import { aSession, renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { Session } from '../../types/Session'
import SessionRow from './SessionRow.vue'
import SessionRowDetail from './SessionRowDetail.vue'

const row = (session: Partial<Session> = {}): VueWrapper =>
  mount(SessionRow, { props: { session: aSession(session) } })

const detailClasses = (wrapper: VueWrapper): string[] =>
  wrapper.getComponent(SessionRowDetail).classes()

const detailText = (wrapper: VueWrapper): string =>
  renderedText(wrapper.getComponent(SessionRowDetail).element)

/**
 * The whole row is a status dot and a title. Working time, longest turn,
 * status text and any tab identifier stay off it — four rounds of mock review
 * took them out, and they are on hover instead.
 */
it('shows a status dot and a title and nothing else', () => {
  expect(
    row({ label: 'Fix permission denied for code command' }).html(),
  ).toMatchInlineSnapshot(
    `"<button type="button" class="group/row row-divider flex w-full flex-col gap-1 px-5 py-2 text-left cursor-pointer hover:bg-neutral-2"><span class="flex w-full items-center gap-3"><span class="h-3 w-3 shrink-0 rounded-full bg-status-done"></span><span class="truncate text-2 title-read min-w-0 flex-1">Fix permission denied for code command</span></span><span class="hidden invisible flex-wrap gap-3 text-0 text-neutral-5 group-hover/list:flex group-hover/row:visible"><span>Done</span><span>Working time 0s</span><span>Longest turn 0s</span></span></button>"`,
  )
})

it('keeps the durations and the status text off the row until it is hovered', () => {
  const rendered = renderedText(
    row({ workingTimeMs: 4_320_000, longestTurnMs: 2_400_000 }).element,
  )

  expect(rendered).toBe('Review attribution mock data test file')
})

/**
 * The reveal is the browser's: the row is a `group/row` and its detail is
 * painted by `group-hover/row:visible` matching. A `ref` toggled by pointer
 * events is never told that the popover was hidden with the pointer over a row,
 * or that the list reordered under a still pointer, so it leaves a resting row
 * showing its detail. `uno.config.test.ts` proves the classes emit real CSS.
 */
it('reveals the detail from the pointer being over the row, not from state', () => {
  const wrapper = row()

  expect(wrapper.classes()).toContain('group/row')
  expect(detailClasses(wrapper)).toContain('invisible')
  expect(detailClasses(wrapper)).toContain('group-hover/row:visible')
})

/**
 * Revealing the detail must move nothing. Nothing the row itself is hovered
 * for may change the detail's display, or the row grows under the pointer and
 * every row below it jumps — which is what moving the mouse down the list used
 * to look like. The space is taken from the list being hovered instead, so the
 * row is the same height whichever row the pointer is on.
 */
it('reserves the detail space from the list, not from its own row', () => {
  const classes = detailClasses(row())

  expect(classes).toContain('group-hover/list:flex')
  expect(
    classes.filter((name) => name.startsWith('group-hover/row:')),
  ).toStrictEqual(['group-hover/row:visible'])
})

it('holds no hover state that pointer events could leave behind', async () => {
  const wrapper = row()
  const atRest = wrapper.html()

  await wrapper.trigger('mouseenter')

  expect(wrapper.html()).toBe(atRest)

  await wrapper.trigger('mouseleave')

  expect(wrapper.html()).toBe(atRest)
})

it('labels both durations and names the status in the detail it reveals', () => {
  const wrapper = row({
    status: 'working',
    workingTimeMs: 4_320_000,
    longestTurnMs: 2_400_000,
  })

  expect(detailText(wrapper)).toBe(
    'Working Working time 1h 12m Longest turn 40m',
  )
})

/**
 * Hover detail is available on every row, including the ones a click cannot
 * take the user anywhere from.
 */
it('reveals the detail on a row that cannot be jumped to', () => {
  const wrapper = row({ jumpable: false, status: 'unknown' })

  expect(detailClasses(wrapper)).toContain('group-hover/row:visible')
  expect(detailText(wrapper)).toBe('Unknown Working time 0s Longest turn 0s')
})

it('asks to mark read and to navigate when a jumpable row is clicked', async () => {
  const wrapper = row({ id: 'abc123', jumpable: true })

  await wrapper.trigger('click')

  expect(wrapper.emitted('read')).toStrictEqual([['abc123']])
  expect(wrapper.emitted('navigate')).toStrictEqual([['abc123']])
})

/**
 * Without the companion extension a click in the popover is the only way to
 * clear unread, so a row that cannot be jumped to stays interactive even
 * though the approved mock draws it inert. It must not navigate and must not
 * fail.
 */
it('asks only to mark read when a row that cannot be jumped to is clicked', async () => {
  const wrapper = row({ id: 'abc123', jumpable: false })

  await wrapper.trigger('click')

  expect(wrapper.emitted('read')).toStrictEqual([['abc123']])
  expect(wrapper.emitted('navigate')).toBeUndefined()
})

/**
 * The user must not expect a click to take them somewhere it cannot. The
 * jumpable row lights up under the pointer and shows the pointer cursor; the
 * one that cannot be jumped to does neither.
 */
it('draws a row that cannot be jumped to differently from one that can', () => {
  const jumpable = row({ jumpable: true }).classes()
  const notJumpable = row({ jumpable: false }).classes()

  expect(jumpable).toContain('hover:bg-neutral-2')
  expect(jumpable).toContain('cursor-pointer')
  expect(notJumpable).not.toContain('hover:bg-neutral-2')
  expect(notJumpable).toContain('cursor-default')
})

/**
 * The app no longer hears which window has focus, so no row claims to be the
 * session the user is in.
 */
it('never marks a row as the session the user is in', () => {
  expect(renderedText(row().element)).not.toContain('you are here')
})
