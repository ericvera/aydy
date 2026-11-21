import { expect, it } from 'vitest'

import type { FocusState } from './focusReporter'
import { FocusReporter } from './focusReporter'

/** Short enough to keep the suite quick, long enough to coalesce a burst. */
const DEBOUNCE_MS = 60

/**
 * A gap inside one burst: shorter than the debounce, longer than none at all.
 */
const STEP_MS = 15

const waited = async (ms: number): Promise<void> =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })

/** Long enough for a debounced report to have gone out. */
const settled = async (): Promise<void> => waited(DEBOUNCE_MS + 90)

interface Recorded {
  reporter: FocusReporter
  sent: FocusState[]
}

/** A reporter recording what it sent, and answering `taken` to each report. */
const reporting = (taken = true): Recorded => {
  const sent: FocusState[] = []
  const reporter = new FocusReporter({
    debounceMs: DEBOUNCE_MS,
    send: (state) => {
      sent.push(state)

      return Promise.resolve(taken)
    },
  })

  return { reporter, sent }
}

const state = (
  focused: boolean,
  activeTabLabel: string | null = null,
): FocusState => ({ focused, activeTabLabel })

it('reports focus and the active tab as two separate facts', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(false, 'Fix the auth flow'))
  await settled()

  expect(sent).toEqual([
    { focused: false, activeTabLabel: 'Fix the auth flow' },
  ])
})

/**
 * Alt-tabbing and clicking through files raise these events several a second,
 * and each report is a round trip. The run is spread over real time, because a
 * burst that arrives in one turn of the loop coalesces whether anything waits
 * for it to settle or not.
 */
it('reports a burst of changes once, as the state it settled on', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  await waited(STEP_MS)
  reporter.report(state(false, 'Fix the auth flow'))
  await waited(STEP_MS)
  reporter.report(state(true, 'Fix the auth flow'))
  await waited(STEP_MS)
  reporter.report(state(false, 'Rewrite the parser'))

  expect(sent).toEqual([])

  await settled()

  expect(sent).toEqual([
    { focused: false, activeTabLabel: 'Rewrite the parser' },
  ])
})

/**
 * The failure this exists to prevent is silent: a window AYDY still believes is
 * in front goes on suppressing its own notifications for as long as it is open.
 */
it('reports losing focus', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()
  reporter.report(state(false, 'Fix the auth flow'))
  await settled()

  expect(sent).toEqual([
    { focused: true, activeTabLabel: 'Fix the auth flow' },
    { focused: false, activeTabLabel: 'Fix the auth flow' },
  ])
})

it('reports the active tab changing while focus stays', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()
  reporter.report(state(true, 'Rewrite the parser'))
  await settled()

  expect(sent).toHaveLength(2)
  expect(sent[1]).toEqual({
    focused: true,
    activeTabLabel: 'Rewrite the parser',
  })
})

it('says nothing when what AYDY holds has not changed', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()
  reporter.report(state(true, 'Fix the auth flow'))
  await settled()

  expect(sent).toHaveLength(1)
})

/**
 * AYDY was not running, or had forgotten this window. Nothing else would ever
 * correct what it holds, so the state has to count as unsaid.
 */
it('says a state again when AYDY did not take it', async () => {
  const { reporter, sent } = reporting(false)

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()
  reporter.report(state(true, 'Fix the auth flow'))
  await settled()

  expect(sent).toHaveLength(2)
})

it('reports a change that arrived while the report before it was in flight', async () => {
  const sent: FocusState[] = []
  let release = (): void => {}
  const reporter = new FocusReporter({
    debounceMs: DEBOUNCE_MS,
    send: async (reported) => {
      sent.push(reported)
      await new Promise<void>((resolve) => {
        release = resolve
      })

      return true
    },
  })

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()
  reporter.report(state(false, 'Fix the auth flow'))
  await settled()
  expect(sent).toHaveLength(1)

  release()
  await settled()

  expect(sent).toEqual([
    { focused: true, activeTabLabel: 'Fix the auth flow' },
    { focused: false, activeTabLabel: 'Fix the auth flow' },
  ])
})

/**
 * AYDY was restarted, or started after this window was. What this window last
 * said went to the AYDY that has gone, and reports are made on change alone,
 * so an editor nobody touches would go unaccounted for until the user moved
 * something.
 */
it('says its state again once AYDY has forgotten it', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()

  reporter.resend()
  await settled()

  expect(sent).toEqual([
    { focused: true, activeTabLabel: 'Fix the auth flow' },
    { focused: true, activeTabLabel: 'Fix the auth flow' },
  ])
})

it('says nothing again for a window that has gone', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  await settled()
  reporter.dispose()

  reporter.resend()
  await settled()

  expect(sent).toHaveLength(1)
})

it('says nothing more once the window it reports on has gone', async () => {
  const { reporter, sent } = reporting()

  reporter.report(state(true, 'Fix the auth flow'))
  reporter.dispose()
  await settled()

  expect(sent).toEqual([])
})
