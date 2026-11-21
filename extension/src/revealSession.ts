/**
 * Bringing the tab that owns a session to the front of this window.
 *
 * There is no supported way to do this. `vscode.TabGroups` can close a tab and
 * nothing else, and the Claude Code extension exports no API — so the only way
 * in is the command it contributes, `claude-vscode.editor.open`, called the way
 * Claude's own webview calls it. Given a session this window already has open,
 * that command takes an internal fast path and reveals the existing panel.
 *
 * **Given a session this window does not have open, the same call creates a
 * panel and resumes the session in it.** That is a second, live Claude session
 * running against the user's work — the most damaging thing this integration
 * can do, and it happens silently. Everything in this file exists to make that
 * impossible:
 *
 * - Only the window that owns the session claims the jump request, by
 *   unlinking it: the window whose folder most specifically contains the
 *   session's launch directory, among the windows published under
 *   `<data>/windows/`.
 * - The tab has to be **here**, matched by the label the request says it
 *   carries, before the command is called at all. A session hosted in the
 *   sidebar is a view rather than a tab and never matches, which is the right
 *   answer: it has no tab to reveal.
 * - The call is only trusted on a Claude Code release the argument order has
 *   been read out of. That order is undocumented and was taken from a minified
 *   bundle that ships roughly weekly.
 * - A tab that opens while the call is made is reported as a duplicate rather
 *   than swallowed, so a guard that failed is visible instead of quietly
 *   doubling the user's sessions.
 */

/**
 * The Claude Code release this reveal path was read out of.
 *
 * In `anthropic.claude-code-2.1.246`, the command is registered as
 * `registerCommand("claude-vscode.editor.open", async (sessionId, prompt,
 * column) => …)` and the panel lookup behind it answers
 * `sessionPanels.get(sessionId)?.reveal()` before it will create anything.
 * Neither the signature nor that fast path is documented.
 */
export const VERIFIED_CLAUDE_CODE_VERSION = '2.1.246'

/**
 * The release line the reveal is trusted on, derived from the version above so
 * that recording a newer one moves both together.
 *
 * A patch a week is the observed cadence, and pinning to a single build would
 * leave the reveal switched off for all but a few days of each release. The
 * line is the compromise: a rename or a reordered argument list is a change of
 * shape, and a change of shape does not usually arrive inside a patch.
 */
const VERIFIED_LINE = `${VERIFIED_CLAUDE_CODE_VERSION.split('.').slice(0, 2).join('.')}.`

/**
 * How long a tab is given to appear after the command was called.
 *
 * The editor raises `onDidChangeTabs` after the command's own promise settles,
 * so a duplicate that was created is only visible a moment later. Long enough
 * to see one, short enough to stay well inside the two seconds AYDY waits for
 * this answer.
 */
const TAB_SETTLE_MS = 120

/**
 * The version an extension manifest declares, or `null` when it declares none.
 *
 * `packageJSON` is whatever the extension shipped, so nothing about its shape
 * can be assumed — the Claude Code integration depends on the internals of a
 * bundle that ships roughly weekly, and a manifest that has changed must leave
 * the reveal refusing (`unexpectedVersion`) rather than throwing.
 */
export const versionOf = (packageJson: unknown): string | null => {
  if (typeof packageJson !== 'object' || packageJson === null) {
    return null
  }

  const version = (packageJson as { version?: unknown }).version

  return typeof version === 'string' && version.length > 0 ? version : null
}

/** What this window did about a reveal. */
export type RevealOutcome =
  'revealed' | 'notPresent' | 'unexpectedVersion' | 'duplicated' | 'failed'

/** One session AYDY wants brought to the front of this window. */
export interface RevealRequest {
  sessionId: string
  /**
   * What AYDY expects the session's tab to read.
   *
   * It is the session's title cut the way the editor cuts it. Matching it is
   * the check this window can make for itself: AYDY knows which window runs the
   * session, and only this window knows what is open in it.
   */
  tabLabel: string
}

/**
 * Everything a reveal touches, so the guards can be driven without an editor.
 */
export interface RevealEnvironment {
  /**
   * The version of the Claude Code extension in this window, or `null` when it
   * is not installed.
   */
  claudeCodeVersion: () => string | null
  /** The labels on every Claude Code tab open in this window. */
  claudeTabs: () => string[]
  /** Calls the editor command that brings a session's own tab forward. */
  open: (sessionId: string) => Promise<void>
  /** Watches for Claude Code tabs opening, until the result is disposed. */
  watchOpenedTabs: (opened: () => void) => { dispose: () => void }
  settleMs?: number
}

/** Whether the reveal path has been checked against `version`. */
export const isVerifiedClaudeCode = (version: string | null): boolean =>
  version !== null && version.startsWith(VERIFIED_LINE)

const waited = async (ms: number): Promise<void> =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms).unref()
  })

/**
 * Brings the tab owning `request.sessionId` to the front of this window, and
 * says what happened.
 *
 * Every answer other than `revealed` leaves the window exactly as it was, which
 * is what makes it safe for AYDY to fall back to raising the window without the
 * tab: a jump that shows the wrong tab is a small failure, and a jump that
 * starts a second copy of the session is not.
 */
export const revealSession = async (
  request: RevealRequest,
  environment: RevealEnvironment,
): Promise<RevealOutcome> => {
  if (!isVerifiedClaudeCode(environment.claudeCodeVersion())) {
    return 'unexpectedVersion'
  }

  if (!environment.claudeTabs().includes(request.tabLabel)) {
    return 'notPresent'
  }

  let opened = 0
  let failed = false
  const watching = environment.watchOpenedTabs(() => {
    opened += 1
  })

  try {
    try {
      await environment.open(request.sessionId)
    } catch {
      // Answered as `failed` rather than thrown: a command that raised may
      // still have opened something, and the jump responder must always be
      // able to write a reply.
      failed = true
    }

    await waited(environment.settleMs ?? TAB_SETTLE_MS)
  } finally {
    watching.dispose()
  }

  // A tab that appeared is the guard above having been wrong, which means a
  // second live copy of the session may now be running. Saying so is the only
  // thing left that can be done about it.
  if (opened > 0) {
    return 'duplicated'
  }

  return failed ? 'failed' : 'revealed'
}
