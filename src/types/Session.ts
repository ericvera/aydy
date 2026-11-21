import type { SessionStatus } from './SessionStatus'

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
  isCurrent: boolean
  workingTimeMs: number
  longestTurnMs: number
  notificationMessage: string | null
}
