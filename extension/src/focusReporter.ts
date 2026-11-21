/**
 * What this window reports about where the user is.
 *
 * The two fields are separate and neither implies the other. A Claude tab left
 * in front of a window sitting behind a browser is exactly the case AYDY exists
 * to notify about, so `activeTabLabel` never means "the user is watching this".
 */
export interface FocusState {
  /** Whether this window currently has focus. */
  focused: boolean
  /**
   * The label on this window's active Claude Code tab, or `null` when no
   * Claude Code tab is active in it — including when the session is hosted in
   * the sidebar, where it is a view and never a tab at all.
   */
  activeTabLabel: string | null
}

/**
 * How long a run of changes is allowed to settle before it is reported.
 *
 * Focus flaps while the user alt-tabs and the active tab changes on every
 * click through a file, and each of those raises an event. Long enough that a
 * run of them costs one report; short enough that the unread clears while the
 * user is still looking at the window.
 */
export const FOCUS_DEBOUNCE_MS = 150

export interface FocusReporterOptions {
  /** Sends one state to AYDY, answering whether AYDY took it. */
  send: (state: FocusState) => Promise<boolean>
  debounceMs?: number
}

/**
 * This window's focus and active tab, reported to AYDY when they settle.
 *
 * One of these belongs to one window: every window runs its own extension host
 * and sees only itself, and exactly one of them reports focus at a time.
 */
export class FocusReporter {
  private readonly send: (state: FocusState) => Promise<boolean>
  private readonly debounceMs: number

  /** The latest state observed, whether or not it has been reported. */
  private pending: FocusState | null = null
  /** The last state AYDY confirmed it took. */
  private sent: FocusState | null = null
  private timer: NodeJS.Timeout | null = null
  private sending = false

  constructor(options: FocusReporterOptions) {
    this.send = options.send
    this.debounceMs = options.debounceMs ?? FOCUS_DEBOUNCE_MS
  }

  /**
   * Records what this window looks like now, to be reported once it has
   * settled.
   */
  report(state: FocusState): void {
    this.pending = state
    this.arm()
  }

  /**
   * Says this window's state again, whether or not it has changed.
   *
   * What a window last said is held by the AYDY that took it, and an AYDY
   * that has just started holds nothing. Reports are made on change alone, so
   * without this a window nobody touches — the ordinary state of an editor
   * left open — stays unreported until the user next moves focus or tabs,
   * which is the heartbeat of silence this reporter exists to avoid.
   */
  resend(): void {
    this.sent = null

    if (this.pending !== null) {
      this.arm()
    }
  }

  /** Stops reporting, which is what a window closing does. */
  dispose(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }

    this.pending = null
  }

  private arm(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer)
    }

    this.timer = setTimeout(() => {
      void this.flush()
    }, this.debounceMs)

    // The extension host outlives this either way, and a pending report cannot
    // be the reason a process refuses to exit.
    this.timer.unref()
  }

  private async flush(): Promise<void> {
    this.timer = null

    const state = this.pending

    if (state === null || this.sending || this.holds(state)) {
      return
    }

    this.sending = true

    try {
      if (await this.send(state)) {
        this.sent = state
      }
      // A report AYDY did not take leaves `sent` alone, so the next change
      // finds this state still unsent and says it again.
    } catch {
      // Reporting again on the next change is the whole of the recovery.
    } finally {
      this.sending = false
    }

    // What this window looks like moved on while the report was in flight, so
    // the report that just went is already out of date. A window that was
    // disposed meanwhile has nothing pending and nothing left to say.
    if (this.pending !== null && this.pending !== state) {
      this.arm()
    }
  }

  /** Whether AYDY holds `state` already, so saying it again says nothing. */
  private holds(state: FocusState): boolean {
    return (
      this.sent !== null &&
      this.sent.focused === state.focused &&
      this.sent.activeTabLabel === state.activeTabLabel
    )
  }
}
