import type { SessionStatus } from '../types/SessionStatus'

const TEXT: Record<SessionStatus, string> = {
  needsInput: 'Needs input',
  working: 'Working',
  done: 'Done',
  unknown: 'Unknown',
}

/**
 * How a status is named to the user. It appears in hover detail only — the row
 * itself carries the status as a coloured dot and nothing else.
 */
export const statusText = (status: SessionStatus): string => TEXT[status]
