/**
 * What came of clicking a session row.
 *
 * A jump writes a request file and waits for the one window that owns the
 * session to answer it, so the app reports what actually happened rather than
 * acknowledging the click: `jumped` is a window that answered and was raised,
 * `noWindow` is a click that went nowhere and says nothing, and the last two
 * are failures the user is told about — no window answered in time, or the
 * reveal opened a tab of its own.
 */
export type JumpOutcome = 'jumped' | 'noWindow' | 'unanswered' | 'duplicated'
