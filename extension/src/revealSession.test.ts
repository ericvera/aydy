import { expect, it } from 'vitest'

import type { RevealEnvironment, RevealOutcome } from './revealSession'
import {
  isVerifiedClaudeCode,
  revealSession,
  VERIFIED_CLAUDE_CODE_VERSION,
} from './revealSession'

/** Short enough to keep the suite quick, long enough for a tab to arrive. */
const SETTLE_MS = 20

const SESSION = 'c4f1d6a2-0f1e-4b1a-9d5e-3a2b1c0d9e8f'

const TAB = 'Models section configura…'

interface Window {
  environment: RevealEnvironment
  /** Every session the editor command was called for. */
  opened: string[]
  /** Opens a Claude tab, the way a reveal that missed its session does. */
  openTab: () => void
  /** Whether the watch on opening tabs is still armed. */
  watching: () => boolean
}

/**
 * A window with `tabs` open, whose editor command opens a tab of its own when
 * `duplicates` is set — which is what Claude Code does when the session it was
 * asked for is not in this window.
 */
const window = (
  tabs: string[],
  options: {
    version?: string | null
    duplicates?: boolean
    throws?: boolean
  } = {},
): Window => {
  const opened: string[] = []
  let notify: (() => void) | null = null

  const openTab = (): void => {
    notify?.()
  }

  const environment: RevealEnvironment = {
    claudeCodeVersion: () =>
      options.version === undefined
        ? VERIFIED_CLAUDE_CODE_VERSION
        : options.version,
    claudeTabs: () => tabs,
    open: async (sessionId: string) => {
      opened.push(sessionId)

      if (options.duplicates === true) {
        openTab()
      }

      if (options.throws === true) {
        throw new Error('the command is not registered')
      }

      return Promise.resolve()
    },
    watchOpenedTabs: (tabOpened: () => void) => {
      notify = tabOpened

      return {
        dispose: () => {
          notify = null
        },
      }
    },
    settleMs: SETTLE_MS,
  }

  return { environment, opened, openTab, watching: () => notify !== null }
}

const reveal = async (window: Window): Promise<RevealOutcome> =>
  revealSession({ sessionId: SESSION, tabLabel: TAB }, window.environment)

it('reveals the session whose tab is open in this window', async () => {
  const editor = window(['Some other session', TAB])

  expect(await reveal(editor)).toBe('revealed')
  expect(editor.opened).toStrictEqual([SESSION])
})

/**
 * The guard the whole feature turns on. Calling the command for a session this
 * window does not have open does not fail — it creates a panel and resumes the
 * session in it, which is a second live copy of the user's work.
 */
it('refuses to call the command when no tab here carries that session', async () => {
  const editor = window(['Some other session'])

  expect(await reveal(editor)).toBe('notPresent')
  expect(editor.opened).toStrictEqual([])
})

/**
 * A session hosted in the sidebar is a view rather than a tab and never appears
 * among the tab groups at all, so there is nothing to reveal — and asking
 * anyway would open a second copy of it in a panel.
 */
it('refuses when this window has no Claude tabs at all', async () => {
  const editor = window([])

  expect(await reveal(editor)).toBe('notPresent')
  expect(editor.opened).toStrictEqual([])
})

/**
 * The argument order is undocumented and was read out of a minified bundle that
 * ships roughly weekly. On a release nobody has checked, raising the window
 * without the tab is the whole of what is safe to do.
 */
it('refuses on a Claude Code release the call has not been checked against', async () => {
  const editor = window([TAB], { version: '3.0.0' })

  expect(await reveal(editor)).toBe('unexpectedVersion')
  expect(editor.opened).toStrictEqual([])
})

it('refuses when Claude Code is not installed in this window', async () => {
  const editor = window([TAB], { version: null })

  expect(await reveal(editor)).toBe('unexpectedVersion')
  expect(editor.opened).toStrictEqual([])
})

/**
 * The guard was wrong and a session has just been opened twice. It is not
 * recoverable from here, so the one thing left is to say so rather than let
 * AYDY report a successful jump over a duplicated session.
 */
it('reports a tab that opened during the call as a duplicate', async () => {
  const editor = window([TAB], { duplicates: true })

  expect(await reveal(editor)).toBe('duplicated')
})

/**
 * A tab the user opened at that exact moment is indistinguishable from one the
 * command opened, and the two failures are not equally bad: a jump wrongly
 * reported as duplicated costs a sentence in the popover, and a duplicate
 * wrongly reported as a jump costs a second live session.
 */
it('reports a tab that opened just after the call as a duplicate', async () => {
  const editor = window([TAB])

  const revealing = reveal(editor)
  editor.openTab()

  expect(await revealing).toBe('duplicated')
})

/**
 * The command being gone is exactly what an extension that renamed it looks
 * like. Letting the rejection escape would have AYDY read it as a window it
 * cannot reach and drop the registration.
 */
it('answers rather than throwing when the command itself fails', async () => {
  const editor = window([TAB], { throws: true })

  expect(await reveal(editor)).toBe('failed')
})

it('stops watching for tabs however the call ended', async () => {
  const revealed = window([TAB])
  const failed = window([TAB], { throws: true })

  await reveal(revealed)
  await reveal(failed)

  expect(revealed.watching()).toBe(false)
  expect(failed.watching()).toBe(false)
})

/**
 * The recorded version is the build the call was read out of; the line is what
 * the reveal is trusted on, because a patch a week would otherwise leave it
 * switched off almost always.
 */
it('trusts the release line it was verified on and nothing else', () => {
  expect(isVerifiedClaudeCode(VERIFIED_CLAUDE_CODE_VERSION)).toBe(true)
  expect(isVerifiedClaudeCode('2.1.9999')).toBe(true)
  expect(isVerifiedClaudeCode('2.2.0')).toBe(false)
  expect(isVerifiedClaudeCode('2.10.1')).toBe(false)
  expect(isVerifiedClaudeCode('3.0.0')).toBe(false)
  expect(isVerifiedClaudeCode('')).toBe(false)
  expect(isVerifiedClaudeCode(null)).toBe(false)
})
