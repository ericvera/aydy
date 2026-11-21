import {
  mkdir,
  readdir,
  readFile,
  realpath,
  stat,
  unlink,
} from 'node:fs/promises'
import { join } from 'node:path'

import { writeAtomically } from './atomicWrite'
import { claudeSessionsDir, seenDir, statusDir } from './aydyPaths'
import { parseStatus, SESSION_ID_PATTERN } from './sessionState'
import type { SessionStatus } from './sessionState'

export interface StoreDirs {
  status?: string
  seen?: string
  sessions?: string
}

const STALE_AFTER_MS = 7 * 24 * 60 * 60 * 1000

const errorCode = (error: unknown): string | undefined =>
  error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined

const listJson = async (dir: string): Promise<string[]> => {
  try {
    return (await readdir(dir)).filter(
      (name) => !name.startsWith('.') && name.endsWith('.json'),
    )
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return []
    }
    throw error
  }
}

const idOf = (fileName: string): string | null => {
  const id = fileName.slice(0, -'.json'.length)
  return SESSION_ID_PATTERN.test(id) ? id : null
}

const parseJson = (text: string): unknown => {
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

const field = (value: unknown, key: string): unknown =>
  typeof value === 'object' && value !== null
    ? (value as Record<string, unknown>)[key]
    : undefined

export const readStatuses = async ({
  status = statusDir(),
}: StoreDirs = {}): Promise<SessionStatus[]> => {
  const statuses: SessionStatus[] = []
  for (const name of await listJson(status)) {
    const path = join(status, name)
    try {
      const [text, info] = await Promise.all([
        readFile(path, 'utf8'),
        stat(path),
      ])
      const parsed = parseStatus(text, info.mtimeMs)
      if (parsed !== null) {
        statuses.push(parsed)
      }
    } catch (error) {
      if (errorCode(error) !== 'ENOENT') {
        throw error
      }
    }
  }
  return statuses
}

export const readSeen = async ({ seen = seenDir() }: StoreDirs = {}): Promise<
  Map<string, number>
> => {
  const result = new Map<string, number>()
  for (const name of await listJson(seen)) {
    const id = idOf(name)
    if (id === null) {
      continue
    }
    let text: string
    try {
      text = await readFile(join(seen, name), 'utf8')
    } catch (error) {
      if (errorCode(error) === 'ENOENT') {
        continue
      }
      throw error
    }
    const seenAt = field(parseJson(text), 'seenAt')
    if (typeof seenAt === 'number' && Number.isFinite(seenAt)) {
      result.set(id, seenAt)
    }
  }
  return result
}

export const writeSeen = async (
  sessionId: string,
  ms: number,
  { seen = seenDir() }: StoreDirs = {},
): Promise<void> => {
  if (!SESSION_ID_PATTERN.test(sessionId)) {
    throw new Error(`Invalid session id: ${sessionId}`)
  }
  await mkdir(seen, { recursive: true })
  await writeAtomically(
    join(seen, `${sessionId}.json`),
    JSON.stringify({ seenAt: ms }),
  )
}

export const isAlive = (pid: unknown): boolean => {
  if (typeof pid !== 'number' || !Number.isInteger(pid) || pid <= 0) {
    return false
  }
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return errorCode(error) === 'EPERM'
  }
}

export const readLiveSessions = async ({
  sessions = claudeSessionsDir(),
}: StoreDirs = {}): Promise<Map<string, string | null> | null> => {
  let names: string[]
  try {
    names = (await readdir(sessions)).filter((name) => name.endsWith('.json'))
  } catch {
    return null
  }
  const live = new Map<string, string | null>()
  for (const name of names) {
    let text: string
    try {
      text = await readFile(join(sessions, name), 'utf8')
    } catch {
      continue
    }
    const file = parseJson(text)
    const sessionId = field(file, 'sessionId')
    const pid = field(file, 'pid')
    if (typeof sessionId !== 'string' || pid === undefined || !isAlive(pid)) {
      continue
    }
    const cwd = field(file, 'cwd')
    const launchCwd = typeof cwd === 'string' && cwd !== '' ? cwd : null
    if (launchCwd !== null || !live.has(sessionId)) {
      live.set(sessionId, launchCwd)
    }
  }
  return live
}

export const resolveCwd = async (
  cwd: string | null,
): Promise<string | null> => {
  if (cwd === null) {
    return null
  }
  try {
    return await realpath(cwd)
  } catch {
    return cwd
  }
}

const pruneDir = async (
  dir: string,
  live: ReadonlyMap<string, unknown>,
  now: number,
): Promise<void> => {
  for (const name of await listJson(dir)) {
    const id = idOf(name)
    if (id === null || live.has(id)) {
      continue
    }
    const path = join(dir, name)
    try {
      if (now - (await stat(path)).mtimeMs > STALE_AFTER_MS) {
        await unlink(path)
      }
    } catch (error) {
      if (errorCode(error) !== 'ENOENT') {
        throw error
      }
    }
  }
}

export const pruneStale = async (
  live: ReadonlyMap<string, unknown> | null,
  now: number,
  { status = statusDir(), seen = seenDir() }: StoreDirs = {},
): Promise<void> => {
  if (live === null) {
    return
  }
  await pruneDir(status, live, now)
  await pruneDir(seen, live, now)
}
