import type { Session } from '../types/Session'

/**
 * A session row with everything at its quietest, so a test states only the
 * fields it is about.
 */
export const aSession = (overrides: Partial<Session> = {}): Session => ({
  id: 'session-1',
  label: 'Review attribution mock data test file',
  repo: null,
  branch: null,
  status: 'done',
  unread: false,
  jumpable: true,
  workingTimeMs: 0,
  longestTurnMs: 0,
  notificationMessage: null,
  ...overrides,
})
