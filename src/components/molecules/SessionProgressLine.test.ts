import { renderedText } from '@/test'
import type { VueWrapper } from '@vue/test-utils'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { WorkSignals } from '../../types/WorkSignals'
import SessionProgressLine from './SessionProgressLine.vue'

const MINUTE_MS = 60_000

const line = (signals: Partial<WorkSignals> = {}): VueWrapper =>
  mount(SessionProgressLine, {
    props: {
      signals: {
        stale: false,
        quietMs: null,
        turnElapsedMs: null,
        step: null,
        eta: null,
        fractionPermille: null,
        ...signals,
      },
    },
  })

const fill = (wrapper: VueWrapper): VueWrapper['element'] | undefined =>
  wrapper.find('.bg-neutral-6').element

it('draws the step and the time left beside a bar at the fraction', () => {
  const wrapper = line({
    turnElapsedMs: 12 * MINUTE_MS,
    step: { done: 4, total: 7 },
    eta: { remainingMs: 6 * MINUTE_MS, reportedAgoMs: 0 },
    fractionPermille: 667,
  })

  expect(renderedText(wrapper.element)).toBe('4/7 · ~6m left')
  expect(wrapper.find('.bg-neutral-3').exists()).toBe(true)
  expect((fill(wrapper) as HTMLElement).style.width).toBe('66.7%')
})

it('draws the time left alone when the report has no step', () => {
  const wrapper = line({
    eta: { remainingMs: 6 * MINUTE_MS, reportedAgoMs: 0 },
    fractionPermille: 500,
  })

  expect(renderedText(wrapper.element)).toBe('~6m left')
})

it('says how far past its ETA a turn has run', () => {
  const wrapper = line({
    step: { done: 4, total: 7 },
    eta: { remainingMs: -2 * MINUTE_MS, reportedAgoMs: 0 },
    fractionPermille: 1000,
  })

  expect(renderedText(wrapper.element)).toBe('4/7 · 2m past ETA')
  expect((fill(wrapper) as HTMLElement).style.width).toBe('100%')
})

it('falls back to the elapsed turn with no bar when nothing was reported', () => {
  const wrapper = line({ turnElapsedMs: 3 * MINUTE_MS })

  expect(renderedText(wrapper.element)).toBe('3m elapsed')
  expect(wrapper.find('.bg-neutral-3').exists()).toBe(false)
})

/**
 * Quiet is a neutral one step brighter than the rest of the line, never a
 * status colour: the dot owns colour, and the hollow dot already says stale.
 */
it('leads a stale line with how long it has been quiet, and drops the bar', () => {
  const wrapper = line({
    stale: true,
    quietMs: 9 * MINUTE_MS,
    turnElapsedMs: 31 * MINUTE_MS,
    fractionPermille: 400,
  })

  expect(renderedText(wrapper.element)).toBe('Quiet 9m 31m elapsed')
  expect(wrapper.find('.bg-neutral-3').exists()).toBe(false)
  expect(wrapper.get('.text-neutral-6').text()).toBe('Quiet 9m')
  expect(wrapper.html()).not.toContain('status')
})

it('lines its text up under the title and stays out of the hover reveal', () => {
  const classes = line({ turnElapsedMs: MINUTE_MS }).classes()

  expect(classes).toContain('pl-5')
  expect(classes.some((name) => name.startsWith('group-hover'))).toBe(false)
  expect(classes).not.toContain('hidden')
})
