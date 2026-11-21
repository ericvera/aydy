import type { PortMove } from './PortMove'
import type { SessionGroup } from './SessionGroup'
import type { StaleList } from './StaleList'

/**
 * The whole list, grouped, ordered and counted by the registry, with what the
 * popover needs to know about the app's own health. Grouping, ordering, both
 * counts and both notices are computed in Rust so the popover and the menu bar
 * icon cannot disagree.
 */
export interface SessionList {
  groups: SessionGroup[]
  sessionCount: number
  unreadCount: number
  portMove: PortMove | null
  stale: StaleList | null
  /**
   * Whether any VS Code window's companion extension is registered right now.
   *
   * It rides on the snapshot because the prompt offering the extension reads
   * it: windows register seconds after the editor starts, so a question asked
   * once when the popover was built would offer the extension to people who
   * already have it.
   */
  extensionConnected: boolean
}
