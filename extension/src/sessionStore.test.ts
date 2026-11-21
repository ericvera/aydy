import {
  mkdirSync,
  mkdtempSync,
  readdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  isAlive,
  pruneStale,
  readLiveSessions,
  readSeen,
  readStatuses,
  resolveCwd,
  writeSeen,
} from './sessionStore'

const DEAD_PID = 2_147_483_647
const DAY_MS = 24 * 60 * 60 * 1000

let root: string
let dirs: { status: string; seen: string; sessions: string }

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'aydy-store-'))
  dirs = {
    status: join(root, 'status'),
    seen: join(root, 'seen'),
    sessions: join(root, 'sessions'),
  }
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

const writeStatus = (id: string, event: string, mtimeMs?: number): void => {
  mkdirSync(dirs.status, { recursive: true })
  const path = join(dirs.status, `${id}.json`)
  writeFileSync(
    path,
    JSON.stringify({ session_id: id, hook_event_name: event, cwd: '/a' }),
  )
  if (mtimeMs !== undefined) {
    utimesSync(path, mtimeMs / 1000, mtimeMs / 1000)
  }
}

const writeSession = (name: string, body: unknown): void => {
  mkdirSync(dirs.sessions, { recursive: true })
  writeFileSync(join(dirs.sessions, name), JSON.stringify(body))
}

it('status and seen files: round-trips statuses and seen times', async () => {
  writeStatus('s1', 'Stop', 1_700_000_000_000)
  await writeSeen('s1', 123, dirs)
  await writeSeen('s2', 456, dirs)

  expect(await readStatuses(dirs)).toEqual([
    { sessionId: 's1', event: 'Stop', cwd: '/a', eventAt: 1_700_000_000_000 },
  ])
  expect(await readSeen(dirs)).toEqual(
    new Map([
      ['s1', 123],
      ['s2', 456],
    ]),
  )
})

it('status and seen files: ignores temp files and unparseable entries', async () => {
  writeStatus('s1', 'Stop')
  writeFileSync(
    join(dirs.status, '.hook.123.tmp'),
    '{"session_id":"t","hook_event_name":"Stop"}',
  )
  writeFileSync(
    join(dirs.status, '.s9.json'),
    '{"session_id":"s9","hook_event_name":"Stop"}',
  )
  writeFileSync(join(dirs.status, 'bad.json'), '{')
  mkdirSync(dirs.seen, { recursive: true })
  writeFileSync(join(dirs.seen, '.s1.json.1.abc.tmp'), '{"seenAt":1}')
  writeFileSync(join(dirs.seen, 'bad.json'), '{"seenAt":"x"}')

  expect((await readStatuses(dirs)).map((s) => s.sessionId)).toEqual(['s1'])
  expect(await readSeen(dirs)).toEqual(new Map())
})

it('status and seen files: returns empty results for missing dirs', async () => {
  expect(await readStatuses(dirs)).toEqual([])
  expect(await readSeen(dirs)).toEqual(new Map())
})

it('status and seen files: refuses a session id that is not a safe file name', async () => {
  await expect(writeSeen('../x', 1, dirs)).rejects.toThrow()
})

it('isAlive: is true for this process', () => {
  expect(isAlive(process.pid)).toBe(true)
})

it.each([DEAD_PID, 0, -1, 1.5, '42', undefined])(
  'isAlive: is false for %s',
  (pid) => {
    expect(isAlive(pid)).toBe(false)
  },
)

it('readLiveSessions: maps live session ids to their launch cwd', async () => {
  writeSession('1.json', { pid: process.pid, sessionId: 'live', cwd: '/a' })
  writeSession('2.json', { pid: process.pid, sessionId: 'nocwd' })
  writeSession('3.json', { pid: DEAD_PID, sessionId: 'dead', cwd: '/d' })
  writeSession('4.json', { pid: DEAD_PID, sessionId: 'live', cwd: '/other' })
  writeSession('5.json', { pid: process.pid, cwd: '/x' })
  writeSession('6.json', { sessionId: 'nopid', cwd: '/x' })
  writeFileSync(join(dirs.sessions, '7.json'), '{')

  expect(await readLiveSessions(dirs)).toEqual(
    new Map([
      ['live', '/a'],
      ['nocwd', null],
    ]),
  )
})

it('readLiveSessions: lets a non-null cwd win between live files sharing an id', async () => {
  writeSession('1.json', { pid: process.pid, sessionId: 'dup', cwd: '/a' })
  writeSession('2.json', { pid: process.pid, sessionId: 'dup' })

  expect(await readLiveSessions(dirs)).toEqual(new Map([['dup', '/a']]))
})

it('readLiveSessions: returns null for a missing sessions dir', async () => {
  expect(await readLiveSessions(dirs)).toBeNull()
})

it('resolveCwd: resolves a symlinked dir to its real path', async () => {
  const real = join(root, 'real')
  const link = join(root, 'link')
  mkdirSync(real)
  symlinkSync(real, link)

  expect(await resolveCwd(link)).toBe(realpathSync(real))
  expect(await resolveCwd(root)).toBe(realpathSync(root))
})

it('resolveCwd: returns a missing path unchanged and keeps null', async () => {
  const missing = join(root, 'gone')
  expect(await resolveCwd(missing)).toBe(missing)
  expect(await resolveCwd(null)).toBeNull()
})

const now = Date.now()

const seed = (): void => {
  writeStatus('oldDead', 'Stop', now - 8 * DAY_MS)
  writeStatus('oldLive', 'Stop', now - 8 * DAY_MS)
  writeStatus('newDead', 'Stop', now - DAY_MS)
  mkdirSync(dirs.seen, { recursive: true })
  for (const [id, age] of [
    ['oldDead', 8],
    ['oldLive', 8],
    ['newDead', 1],
  ] as const) {
    const path = join(dirs.seen, `${id}.json`)
    writeFileSync(path, '{"seenAt":1}')
    utimesSync(path, (now - age * DAY_MS) / 1000, (now - age * DAY_MS) / 1000)
  }
}

it('pruneStale: removes only old files of sessions that are not live', async () => {
  seed()
  await pruneStale(new Map([['oldLive', '/a']]), now, dirs)

  expect(readdirSync(dirs.status).sort()).toEqual([
    'newDead.json',
    'oldLive.json',
  ])
  expect(readdirSync(dirs.seen).sort()).toEqual([
    'newDead.json',
    'oldLive.json',
  ])
})

it('pruneStale: does nothing when liveness is unknown', async () => {
  seed()
  await pruneStale(null, now, dirs)

  expect(readdirSync(dirs.status)).toHaveLength(3)
  expect(readdirSync(dirs.seen)).toHaveLength(3)
})
