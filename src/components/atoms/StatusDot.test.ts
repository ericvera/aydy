import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { SessionStatus } from '../../types/SessionStatus'
import StatusDot from './StatusDot.vue'

const dot = (status: SessionStatus): string =>
  mount(StatusDot, { props: { status } }).html()

it('is amber when the session needs input', () => {
  expect(dot('needsInput')).toMatchInlineSnapshot(
    `"<span class="h-3 w-3 shrink-0 rounded-full bg-status-needs-input"></span>"`,
  )
})

it('is blue while the session is working', () => {
  expect(dot('working')).toMatchInlineSnapshot(
    `"<span class="h-3 w-3 shrink-0 rounded-full bg-status-working"></span>"`,
  )
})

it('is green when the session is done', () => {
  expect(dot('done')).toMatchInlineSnapshot(
    `"<span class="h-3 w-3 shrink-0 rounded-full bg-status-done"></span>"`,
  )
})

it('is the idle grey when nothing is known about the session', () => {
  expect(dot('unknown')).toMatchInlineSnapshot(
    `"<span class="h-3 w-3 shrink-0 rounded-full bg-status-idle"></span>"`,
  )
})

/**
 * The dot is the only thing in a row that carries colour, which is what keeps
 * it a reliable read of session state. Four statuses, four colours, none of
 * them shared.
 */
it('gives every status a colour of its own', () => {
  const statuses: SessionStatus[] = ['needsInput', 'working', 'done', 'unknown']
  const colors = statuses.map(
    (status) => /bg-status-[a-z-]+/.exec(dot(status))?.[0],
  )

  expect(new Set(colors).size).toBe(statuses.length)
  expect(colors).not.toContain(undefined)
})
