/**
 * What Rust knows about a Working session's progress. Durations are whole
 * milliseconds, a negative `eta.remainingMs` is past the ETA, and
 * `fractionPermille` runs 0 to 1000. A session carries `null` instead unless
 * it is Working.
 */
export interface WorkSignals {
  stale: boolean
  quietMs: number | null
  turnElapsedMs: number | null
  step: { done: number; total: number } | null
  eta: { remainingMs: number; reportedAgoMs: number } | null
  fractionPermille: number | null
}
