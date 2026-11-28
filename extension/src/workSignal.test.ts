import { expect, it } from 'vitest'

import type { SessionEvent, SessionStatus } from './sessionState'
import {
  formatMinutes,
  mostUrgent,
  nextTurnStart,
  parseProgress,
  sessionSignal,
  STALE_AFTER_MS,
  titleSignal,
} from './workSignal'
import type {
  ProgressReport,
  SessionActivity,
  SessionSignal,
} from './workSignal'

const MINUTE = 60 * 1000
const START = 1_700_000_000_000

const status = (
  event: SessionEvent,
  eventAt = START,
  subagentsRunning = false,
): SessionStatus => ({
  sessionId: 's1',
  event,
  cwd: '/a',
  transcriptPath: '/p/s1.jsonl',
  eventAt,
  subagentsRunning,
  informational: false,
})

const report = (
  reportedAt: number,
  etaMs: number,
  step: ProgressReport['step'] = null,
): ProgressReport => ({ reportedAt, etaMs, step })

const activity = (
  overrides: Partial<SessionActivity> = {},
): SessionActivity => ({
  status: status('UserPromptSubmit'),
  turnStartedAt: START,
  transcriptWrittenAt: START,
  progress: null,
  ...overrides,
})

const signalAt = (
  overrides: Partial<SessionActivity>,
  now: number,
): SessionSignal => {
  const result = sessionSignal(activity(overrides), now)
  if (result === null) {
    throw new Error('expected a signal')
  }
  return result
}

const signal = (overrides: Partial<SessionSignal> = {}): SessionSignal => ({
  stale: false,
  quietMs: 0,
  elapsedMs: null,
  step: null,
  etaRemainingMs: null,
  ...overrides,
})

it('nextTurnStart: a prompt starts the turn at its event time', () => {
  expect(nextTurnStart(null, status('UserPromptSubmit', 10))).toBe(10)
})

it('nextTurnStart: a second prompt keeps the open turn start', () => {
  expect(nextTurnStart(10, status('UserPromptSubmit', 20))).toBe(10)
})

it('nextTurnStart: a Stop with subagents running keeps the open turn start', () => {
  expect(nextTurnStart(10, status('Stop', 20, true))).toBe(10)
})

it('nextTurnStart: a Stop with subagents running and no open turn starts at its event time', () => {
  expect(nextTurnStart(null, status('Stop', 20, true))).toBe(20)
})

it('nextTurnStart: a Stop without subagents ends the turn', () => {
  expect(nextTurnStart(10, status('Stop', 20))).toBeNull()
})

it.each(['Notification', 'SessionStart'] as const)(
  'nextTurnStart: a %s keeps the previous value',
  (event) => {
    expect(nextTurnStart(10, status(event, 20))).toBe(10)
    expect(nextTurnStart(null, status(event, 20))).toBeNull()
  },
)

it('nextTurnStart: is idempotent on a repeated status', () => {
  for (const repeated of [
    status('UserPromptSubmit', 10),
    status('Stop', 20, true),
    status('Stop', 20),
    status('Notification', 20),
  ]) {
    const once = nextTurnStart(5, repeated)
    expect(nextTurnStart(once, repeated)).toBe(once)
  }
})

const prompt = START
const stop = START + 5 * MINUTE
const turnStartedAt = [
  status('UserPromptSubmit', prompt),
  status('Stop', stop, true),
].reduce<number | null>(nextTurnStart, null)
const at = (progress: ProgressReport): SessionSignal =>
  signalAt(
    {
      status: status('Stop', stop, true),
      turnStartedAt,
      transcriptWrittenAt: stop,
      progress,
    },
    stop + MINUTE,
  )

it('a turn that stops with subagents running: keeps the prompt as the turn start', () => {
  expect(turnStartedAt).toBe(prompt)
})

it('a turn that stops with subagents running: accepts a report written after the prompt', () => {
  expect(
    at(report(prompt + 2 * MINUTE, 10 * MINUTE, { done: 2, total: 4 })).step,
  ).toEqual({ done: 2, total: 4 })
})

it('a turn that stops with subagents running: rejects a report from the previous turn', () => {
  const rejected = at(report(prompt - 5 * MINUTE, 10 * MINUTE))
  expect(rejected.etaRemainingMs).toBeNull()
  expect(rejected.step).toBeNull()
})

it('sessionSignal: is null unless the session is working', () => {
  expect(sessionSignal(activity({ status: status('Stop') }), START)).toBeNull()
  expect(
    sessionSignal(activity({ status: status('Stop', START, true) }), START),
  ).not.toBeNull()
})

it('sessionSignal: is not stale at 4:59 of quiet', () => {
  expect(signalAt({}, START + STALE_AFTER_MS - 1000).stale).toBe(false)
})

it('sessionSignal: is stale at exactly five minutes of quiet', () => {
  const result = signalAt({}, START + STALE_AFTER_MS)
  expect(result.stale).toBe(true)
  expect(result.quietMs).toBe(STALE_AFTER_MS)
})

it('sessionSignal: an unknown transcript is never stale', () => {
  const result = signalAt({ transcriptWrittenAt: null }, START + 600 * MINUTE)
  expect(result.quietMs).toBeNull()
  expect(result.stale).toBe(false)
})

it('sessionSignal: a status event newer than the transcript resets quiet', () => {
  const result = signalAt(
    { status: status('UserPromptSubmit', START + 4 * MINUTE) },
    START + 6 * MINUTE,
  )
  expect(result.quietMs).toBe(2 * MINUTE)
  expect(result.stale).toBe(false)
})

it('sessionSignal: subagent activity counts through the transcript written-at', () => {
  const result = signalAt(
    { transcriptWrittenAt: START + 4 * MINUTE },
    START + 6 * MINUTE,
  )
  expect(result.quietMs).toBe(2 * MINUTE)
})

it('sessionSignal: an accepted report counts as activity', () => {
  const result = signalAt(
    { progress: report(START + 4 * MINUTE, 10 * MINUTE) },
    START + 6 * MINUTE,
  )
  expect(result.quietMs).toBe(2 * MINUTE)
})

it('sessionSignal: ignores a report from before the turn start', () => {
  const turnStart = START + 10 * MINUTE
  const result = signalAt(
    {
      turnStartedAt: turnStart,
      progress: report(START + 5 * MINUTE, 30 * MINUTE, {
        done: 2,
        total: 4,
      }),
    },
    turnStart + MINUTE,
  )
  expect(result.step).toBeNull()
  expect(result.etaRemainingMs).toBeNull()
  expect(result.quietMs).toBe(11 * MINUTE)
  expect(result.stale).toBe(true)
})

it('sessionSignal: accepts any report when the turn start is unknown', () => {
  const result = signalAt(
    { turnStartedAt: null, progress: report(START - MINUTE, 10 * MINUTE) },
    START,
  )
  expect(result.etaRemainingMs).toBe(9 * MINUTE)
  expect(result.elapsedMs).toBeNull()
})

it("sessionSignal: takes the accepted report's step as the step", () => {
  expect(
    signalAt(
      { progress: report(START, 10 * MINUTE, { done: 3, total: 7 }) },
      START + MINUTE,
    ).step,
  ).toEqual({ done: 3, total: 7 })
})

it('sessionSignal: has no step without a report', () => {
  const result = signalAt({}, START + MINUTE)
  expect(result.step).toBeNull()
  expect(result.elapsedMs).toBe(MINUTE)
})

it('sessionSignal: counts the ETA down and goes negative when overdue', () => {
  const reported = { progress: report(START, 10 * MINUTE) }
  expect(signalAt(reported, START + 4 * MINUTE).etaRemainingMs).toBe(6 * MINUTE)
  expect(signalAt(reported, START + 12 * MINUTE).etaRemainingMs).toBe(
    -2 * MINUTE,
  )
})

it('parseProgress: reads the ETA, the step and the report time', () => {
  expect(
    parseProgress('{"etaSeconds":360,"step":{"done":4,"total":7}}', 42),
  ).toEqual({ etaMs: 360_000, step: { done: 4, total: 7 }, reportedAt: 42 })
  expect(parseProgress('{"etaSeconds":0}', 1)).toEqual({
    etaMs: 0,
    step: null,
    reportedAt: 1,
  })
})

it.each([
  ['over', '{"done":5,"total":4}'],
  ['zero total', '{"done":0,"total":0}'],
  ['negative', '{"done":-1,"total":4}'],
  ['fraction', '{"done":1.5,"total":4}'],
  ['shape', '"halfway"'],
])('parseProgress: drops a %s step but keeps the report', (_label, step) => {
  expect(parseProgress(`{"etaSeconds":60,"step":${step}}`, 1)).toEqual({
    etaMs: 60_000,
    step: null,
    reportedAt: 1,
  })
})

it.each([
  ['missing ETA', '{"step":{"done":1,"total":2}}'],
  ['negative ETA', '{"etaSeconds":-5}'],
  ['fractional ETA', '{"etaSeconds":1.5}'],
  ['string ETA', '{"etaSeconds":"60"}'],
  ['bad JSON', '{not json'],
  ['non-object JSON', '60'],
])('parseProgress: skips a %s', (_label, text) => {
  expect(parseProgress(text, 1)).toBeNull()
})

it('parseProgress: caps the ETA at a day', () => {
  expect(parseProgress('{"etaSeconds":999999999}', 1)?.etaMs).toBe(
    24 * 60 * MINUTE,
  )
})

it('mostUrgent: is null without signals', () => {
  expect(mostUrgent([])).toBeNull()
})

it('mostUrgent: stale beats an ETA', () => {
  const stale = signal({ stale: true, quietMs: 6 * MINUTE })
  const eta = signal({ etaRemainingMs: -10 * MINUTE })
  expect(mostUrgent([eta, stale])).toBe(stale)
})

it('mostUrgent: the longest quiet wins among stale', () => {
  const short = signal({ stale: true, quietMs: 6 * MINUTE })
  const long = signal({ stale: true, quietMs: 9 * MINUTE })
  expect(mostUrgent([short, long])).toBe(long)
})

it('mostUrgent: the shortest ETA wins, overdue first', () => {
  const later = signal({ etaRemainingMs: 6 * MINUTE })
  const sooner = signal({ etaRemainingMs: MINUTE })
  const overdue = signal({ etaRemainingMs: -2 * MINUTE })
  expect(mostUrgent([later, sooner])).toBe(sooner)
  expect(mostUrgent([later, sooner, overdue])).toBe(overdue)
})

it('mostUrgent: an ETA beats elapsed time', () => {
  const elapsed = signal({ elapsedMs: 60 * MINUTE })
  const eta = signal({ etaRemainingMs: 30 * MINUTE })
  expect(mostUrgent([elapsed, eta])).toBe(eta)
})

it('mostUrgent: the longest elapsed wins without an ETA', () => {
  const short = signal({ elapsedMs: MINUTE })
  const long = signal({ elapsedMs: 3 * MINUTE })
  const nothing = signal()
  expect(mostUrgent([nothing, short, long])).toBe(long)
})

it('mostUrgent: ties keep input order', () => {
  const first = signal({ etaRemainingMs: MINUTE })
  const second = signal({ etaRemainingMs: MINUTE })
  expect(mostUrgent([first, second])).toBe(first)
})

it.each([
  [
    signal({
      step: { done: 4, total: 7 },
      etaRemainingMs: 6 * MINUTE,
      elapsedMs: MINUTE,
    }),
    { stale: false, text: '4/7 · ~6m' },
  ],
  [signal({ etaRemainingMs: 6 * MINUTE }), { stale: false, text: '~6m' }],
  [signal({ elapsedMs: 3 * MINUTE }), { stale: false, text: '3m' }],
  [
    signal({ etaRemainingMs: -2 * MINUTE, elapsedMs: 9 * MINUTE }),
    { stale: false, text: '2m over' },
  ],
  [
    signal({
      stale: true,
      quietMs: 9 * MINUTE,
      etaRemainingMs: 6 * MINUTE,
      step: { done: 4, total: 7 },
    }),
    { stale: true, text: 'quiet 9m' },
  ],
  [signal({ elapsedMs: 30 * 1000 }), { stale: false, text: '<1m' }],
  [signal({ elapsedMs: 70 * MINUTE }), { stale: false, text: '1h 10m' }],
])('titleSignal: %j reads %j', (input, expected) => {
  expect(titleSignal(input)).toEqual(expected)
})

it('titleSignal: is null when there is nothing to say', () => {
  expect(titleSignal(signal())).toBeNull()
})

it.each([
  [0, '<1m'],
  [59 * 1000, '<1m'],
  [60 * 1000, '1m'],
  [59 * MINUTE + 59 * 1000, '59m'],
  [60 * MINUTE, '1h 0m'],
  [70 * MINUTE, '1h 10m'],
])('formatMinutes: %i ms reads %s', (ms, text) => {
  expect(formatMinutes(ms)).toBe(text)
})
