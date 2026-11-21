import type { SessionGroup } from './SessionGroup'

/**
 * The whole list, grouped, ordered and counted by the registry. Grouping,
 * ordering and both counts are computed in Rust so the popover and the menu bar
 * icon cannot disagree.
 */
export interface SessionList {
  groups: SessionGroup[]
  sessionCount: number
  unreadCount: number
}
