import type { SessionStatus } from './SessionStatus'
import type { WorkSignals } from './WorkSignals'

/**
 * One session row. Durations are whole milliseconds.
 */
export interface Session {
  id: string
  label: string
  repo: string | null
  branch: string | null
  status: SessionStatus
  unread: boolean
  jumpable: boolean
  workingTimeMs: number
  longestTurnMs: number
  notificationMessage: string | null
  signals: WorkSignals | null
}
