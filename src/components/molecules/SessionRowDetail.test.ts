import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { SessionStatus } from '../../types/SessionStatus'
import SessionRowDetail from './SessionRowDetail.vue'

const detail = (
  status: SessionStatus,
  workingTimeMs: number,
  longestTurnMs: number,
): string =>
  renderedText(
    mount(SessionRowDetail, {
      props: { status, workingTimeMs, longestTurnMs },
    }).element,
  )

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
