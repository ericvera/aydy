/**
 * A session's derived status, as Rust names it on the wire.
 */
export type SessionStatus = 'needsInput' | 'working' | 'done' | 'unknown'
