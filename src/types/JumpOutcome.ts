/**
 * What came of clicking a session row.
 *
 * Every mechanism behind a jump fails silently, so the app answers with what
 * actually happened rather than acknowledging the click: `jumped` was confirmed
 * by the window itself, `raised` is the best a machine without the companion
 * extension can know, `noWindow` is a click that went nowhere and says nothing,
 * and the last two are failures the user is told about.
 */
export type JumpOutcome =
  'jumped' | 'raised' | 'noWindow' | 'unconfirmed' | 'duplicated'
