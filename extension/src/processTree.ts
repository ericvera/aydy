import { execFile } from 'node:child_process'
import { readdir, readFile } from 'node:fs/promises'
import { join } from 'node:path'

import { claudeSessionsDir } from './aydyPaths'
import { SESSION_ID_PATTERN } from './sessionState'

/** Parses `ps -axo pid=,ppid=` into a child → parent map. */
export const parseParents = (text: string): Map<number, number> => {
  const parents = new Map<number, number>()
  for (const line of text.split('\n')) {
    const [pid, ppid] = line.trim().split(/\s+/).map(Number)
    if (Number.isInteger(pid) && Number.isInteger(ppid)) {
      parents.set(pid as number, ppid as number)
    }
  }
  return parents
}

/** Whether `pid` is `root` or descends from it. */
export const descendsFrom = (
  pid: number,
  root: number,
  parents: ReadonlyMap<number, number>,
): boolean => {
  const visited = new Set<number>()
  let current: number | undefined = pid
  while (current !== undefined && current > 1 && !visited.has(current)) {
    if (current === root) {
      return true
    }
    visited.add(current)
    current = parents.get(current)
  }
  return false
}

/** Reads every process's parent from `ps`. */
export const readParents = (): Promise<Map<number, number>> =>
  new Promise((resolve, reject) => {
    execFile('ps', ['-axo', 'pid=,ppid='], (error, stdout) => {
      if (error !== null) {
        reject(error)
        return
      }
      resolve(parseParents(stdout))
    })
  })

const isAlive = (pid: number): boolean => {
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return error instanceof Error && 'code' in error && error.code === 'EPERM'
  }
}

export interface LiveSession {
  pid: number
  sessionId: string
}

/** The live Claude Code sessions Claude Code has registered. */
export const readLiveSessions = async (
  dir: string = claudeSessionsDir(),
): Promise<LiveSession[]> => {
  let names: string[]
  try {
    names = (await readdir(dir)).filter((name) => name.endsWith('.json'))
  } catch {
    return []
  }
  const sessions: LiveSession[] = []
  for (const name of names) {
    try {
      const parsed: unknown = JSON.parse(
        await readFile(join(dir, name), 'utf8'),
      )
      if (typeof parsed !== 'object' || parsed === null) {
        continue
      }
      const pid = 'pid' in parsed ? parsed.pid : undefined
      const sessionId = 'sessionId' in parsed ? parsed.sessionId : undefined
      if (
        typeof pid === 'number' &&
        typeof sessionId === 'string' &&
        SESSION_ID_PATTERN.test(sessionId) &&
        isAlive(pid)
      ) {
        sessions.push({ pid, sessionId })
      }
    } catch {
      continue
    }
  }
  return sessions
}

/**
 * The ids of the sessions running under `root` — this window's extension host,
 * which is where the Claude Code extension starts its sessions.
 */
export const sessionIdsUnder = (
  sessions: readonly LiveSession[],
  root: number,
  parents: ReadonlyMap<number, number>,
): string[] =>
  sessions
    .filter((session) => descendsFrom(session.pid, root, parents))
    .map((session) => session.sessionId)

/** Whether any process has `pid` as its parent. */
export const hasChildren = (
  pid: number,
  parents: ReadonlyMap<number, number>,
): boolean => {
  for (const parent of parents.values()) {
    if (parent === pid) {
      return true
    }
  }
  return false
}
