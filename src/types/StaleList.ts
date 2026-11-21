/**
 * What the list was when the app stopped being able to receive events.
 *
 * Present only while it cannot receive them, which cannot happen before its
 * hooks exist — nothing is being sent then, so nothing is being lost.
 */
export interface StaleList {
  sessionCount: number
  lastSeenAgoMs: number
}
