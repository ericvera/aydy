import { expect, it, vi } from 'vitest'

import type { JumpOutcome } from '../types/JumpOutcome'

interface Answers {
  outcome: JumpOutcome
  fails: boolean
}

const { invoked, answers } = vi.hoisted(
  (): {
    invoked: { command: string; args: Record<string, unknown> | undefined }[]
    answers: Answers
  } => ({
    invoked: [],
    answers: { outcome: 'jumped', fails: false },
  }),
)

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (
    command: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> => {
    invoked.push({ command, args })

    if (answers.fails) {
      throw new Error('the command is not registered')
    }

    return answers.outcome
  },
}))

/**
 * A fresh module, since the failure is module scoped so that the list and the
 * notice above it are looking at one answer.
 */
const load = async (
  outcome: JumpOutcome = 'jumped',
  fails = false,
): Promise<ReturnType<(typeof import('./useJump'))['useJump']>> => {
  invoked.length = 0
  answers.outcome = outcome
  answers.fails = fails
  vi.resetModules()

  const { useJump } = await import('./useJump')

  return useJump()
}

it('asks the app to take the user to a session', async () => {
  const jumping = await load()

  expect(await jumping.jump('0d4b9a1c')).toBe(true)
  expect(invoked).toStrictEqual([
    { command: 'jump_to_session', args: { sessionId: '0d4b9a1c' } },
  ])
  expect(jumping.failure.value).toBe(null)
})

/**
 * The click goes nowhere and says nothing, which is what a row that cannot be
 * jumped to has always done. Reporting an error here would make an ordinary
 * state look broken.
 */
it('does not navigate and does not complain when no window was found', async () => {
  const jumping = await load('noWindow')

  expect(await jumping.jump('0d4b9a1c')).toBe(false)
  expect(jumping.failure.value).toBe(null)
})

it('reports a jump no window answered', async () => {
  const jumping = await load('unanswered')

  expect(await jumping.jump('0d4b9a1c')).toBe(false)
  expect(jumping.failure.value).toBe('unanswered')
})

it('reports a reveal that opened a tab of its own', async () => {
  const jumping = await load('duplicated')

  expect(await jumping.jump('0d4b9a1c')).toBe(false)
  expect(jumping.failure.value).toBe('duplicated')
})

/**
 * A jump that could not even be asked for is a jump no window answered.
 * Claiming otherwise is the silent success this whole path exists to avoid.
 */
it('reports a jump the app never answered', async () => {
  const jumping = await load('jumped', true)

  expect(await jumping.jump('0d4b9a1c')).toBe(false)
  expect(jumping.failure.value).toBe('unanswered')
})

it('clears the last failure when the next jump is asked for', async () => {
  const jumping = await load('unanswered')
  await jumping.jump('0d4b9a1c')
  expect(jumping.failure.value).toBe('unanswered')

  answers.outcome = 'jumped'
  await jumping.jump('0d4b9a1c')

  expect(jumping.failure.value).toBe(null)
})

it('clears the last failure when it is dismissed', async () => {
  const jumping = await load('unanswered')
  await jumping.jump('0d4b9a1c')

  jumping.dismiss()

  expect(jumping.failure.value).toBe(null)
})

/**
 * The popover is still up and still clickable while a jump waits, so a row can
 * be clicked again before the first click has answered. Both clicks asked for
 * the one thing, and asking the app twice would reveal the tab a second time
 * and raise the window twice over.
 */
it('asks for one jump when a row is clicked again before the first answers', async () => {
  const jumping = await load()

  const clicked = jumping.jump('0d4b9a1c')
  const clickedAgain = jumping.jump('0d4b9a1c')

  expect(await clicked).toBe(true)
  expect(await clickedAgain).toBe(true)
  expect(invoked).toStrictEqual([
    { command: 'jump_to_session', args: { sessionId: '0d4b9a1c' } },
  ])
})

/** A row that has been jumped to once is not left unclickable. */
it('asks again for a row whose jump has already answered', async () => {
  const jumping = await load()
  await jumping.jump('0d4b9a1c')

  await jumping.jump('0d4b9a1c')

  expect(invoked).toHaveLength(2)
})

/**
 * Only the row already jumping is the same jump. Every other row is its own,
 * and a click on one is not made to wait behind another row's jump.
 */
it('makes a click on another row a jump of its own', async () => {
  const jumping = await load()

  const clicked = jumping.jump('0d4b9a1c')
  const other = jumping.jump('7f2e5b30')

  expect(await clicked).toBe(true)
  expect(await other).toBe(true)
  expect(invoked.map(({ args }) => args?.['sessionId'])).toStrictEqual([
    '0d4b9a1c',
    '7f2e5b30',
  ])
})
