import { execFile } from 'node:child_process'
import { readdir, readFile } from 'node:fs/promises'
import { join } from 'node:path'

import { claudeSessionsDir } from './aydyPaths'

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

const readParents = (): Promise<Map<number, number>> =>
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

/** The pids of the live Claude Code sessions Claude Code has registered. */
export const readSessionPids = async (
  dir: string = claudeSessionsDir(),
): Promise<number[]> => {
  let names: string[]
  try {
    names = (await readdir(dir)).filter((name) => name.endsWith('.json'))
  } catch {
    return []
  }
  const pids: number[] = []
  for (const name of names) {
    try {
      const parsed: unknown = JSON.parse(
        await readFile(join(dir, name), 'utf8'),
      )
      const pid =
        typeof parsed === 'object' && parsed !== null && 'pid' in parsed
          ? parsed.pid
          : undefined
      if (typeof pid === 'number' && isAlive(pid)) {
        pids.push(pid)
      }
    } catch {
      continue
    }
  }
  return pids
}

/**
 * How many live Claude Code sessions run under `root` — this window's extension
 * host, which is where the Claude Code extension starts its sessions.
 */
export const sessionsUnder = async (root: number): Promise<number> => {
  const pids = await readSessionPids()
  if (pids.length === 0) {
    return 0
  }
  const parents = await readParents()
  return pids.filter((pid) => descendsFrom(pid, root, parents)).length
}
