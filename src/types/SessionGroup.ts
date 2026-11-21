import type { Session } from './Session'
import type { SessionGroupKind } from './SessionGroupKind'

/**
 * One group of sessions, already ordered by Rust.
 */
export interface SessionGroup {
  kind: SessionGroupKind
  repo: string | null
  branch: string | null
  sessions: Session[]
}
