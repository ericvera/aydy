import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { SessionStatus } from '../../types/SessionStatus'
import type { WorkSignals } from '../../types/WorkSignals'
import SessionRowDetail from './SessionRowDetail.vue'

const detail = (
  status: SessionStatus,
  workingTimeMs: number,
  longestTurnMs: number,
  signals: WorkSignals | null = null,
): string =>
  renderedText(
    mount(SessionRowDetail, {
      props: { status, workingTimeMs, longestTurnMs, signals },
    }).element,
  )

const someSignals = (signals: Partial<WorkSignals>): WorkSignals => ({
  stale: false,
  quietMs: null,
  turnElapsedMs: null,
  step: null,
  eta: null,
  fractionPermille: null,
  ...signals,
})

/**
 * Both durations carry a label of their own, so working time — which excludes
 * every wait — cannot be read as the elapsed time a turn took.
 */
it('labels both durations', () => {
  expect(detail('working', 4_320_000, 2_400_000)).toBe(
    'Working Working time 1h 12m Longest turn 40m',
  )
})

it('names the status in words', () => {
  expect(detail('needsInput', 0, 0)).toBe(
    'Needs input Working time 0s Longest turn 0s',
  )
})

it('writes every status in words of its own', () => {
  const statuses: SessionStatus[] = ['needsInput', 'working', 'done', 'unknown']
  const named = statuses.map(
    (status) => detail(status, 0, 0).split(' Working time')[0],
  )

  expect(named).toStrictEqual(['Needs input', 'Working', 'Done', 'Unknown'])
})

it('renders the two durations independently of each other', () => {
  expect(detail('done', 0, 5_000)).toBe('Done Working time 0s Longest turn 5s')
})

it('tells a working row how long its turn has run and when its ETA came', () => {
  const signals = someSignals({
    turnElapsedMs: 720_000,
    eta: { remainingMs: 360_000, reportedAgoMs: 60_000 },
  })

  expect(detail('working', 2_280_000, 840_000, signals)).toBe(
    'Working This turn 12m ETA reported 1m ago Working time 38m Longest turn 14m',
  )
})

it('says what quiet means right after the status of a stale row', () => {
  const signals = someSignals({
    stale: true,
    quietMs: 540_000,
    turnElapsedMs: 1_860_000,
  })

  expect(detail('working', 3_120_000, 840_000, signals)).toBe(
    'Working Quiet 9m · nothing written to the transcript This turn 31m No ETA reported Working time 52m Longest turn 14m',
  )
})

it('draws the quiet part brighter in a neutral, never a status colour', () => {
  const wrapper = mount(SessionRowDetail, {
    props: {
      status: 'working',
      workingTimeMs: 0,
      longestTurnMs: 0,
      signals: someSignals({ stale: true, quietMs: 540_000 }),
    },
  })

  expect(wrapper.findAll('span')[2]?.classes()).toStrictEqual([
    'text-neutral-6',
  ])
})

it('says no ETA was reported on a working row the agent has not reported on', () => {
  const signals = someSignals({ turnElapsedMs: 180_000 })

  expect(detail('working', 180_000, 0, signals)).toBe(
    'Working This turn 3m No ETA reported Working time 3m Longest turn 0s',
  )
})

it('leaves a row with no signals as it was', () => {
  expect(detail('done', 0, 5_000, null)).toBe(
    'Done Working time 0s Longest turn 5s',
  )
})

it('shows no signals on a row that is not working', () => {
  const signals = someSignals({ stale: true, turnElapsedMs: 180_000 })

  expect(detail('done', 0, 5_000, signals)).toBe(
    'Done Working time 0s Longest turn 5s',
  )
})

it('keeps the reveal classes whatever signals the row carries', () => {
  const wrapper = mount(SessionRowDetail, {
    props: {
      status: 'working',
      workingTimeMs: 0,
      longestTurnMs: 0,
      signals: someSignals({ stale: true, quietMs: 540_000 }),
    },
  })

  expect(wrapper.attributes('class')).toBe(
    'hidden invisible flex-wrap gap-3 text-0 text-neutral-5 group-hover/list:flex group-hover/row:visible',
  )
})
