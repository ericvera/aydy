/**
 * What a group of sessions is grouped by: a known repository, a project not yet
 * identified, or a session that cannot be jumped to.
 */
export type SessionGroupKind = 'project' | 'unidentified' | 'notJumpable'
