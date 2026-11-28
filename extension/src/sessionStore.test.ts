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
import { dirname, join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  isAlive,
  pruneStale,
  readLiveSessions,
  readProgress,
  readSeen,
  readStatuses,
  resolveCwd,
  transcriptActivity,
  writeSeen,
} from './sessionStore'

const DEAD_PID = 2_147_483_647
const DAY_MS = 24 * 60 * 60 * 1000

let root: string
let dirs: { status: string; seen: string; sessions: string; progress: string }

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'aydy-store-'))
  dirs = {
    status: join(root, 'status'),
    seen: join(root, 'seen'),
    sessions: join(root, 'sessions'),
    progress: join(root, 'progress'),
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

const writeAt = (path: string, body: string, mtimeMs: number): void => {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, body)
  utimesSync(path, mtimeMs / 1000, mtimeMs / 1000)
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
    {
      sessionId: 's1',
      event: 'Stop',
      cwd: '/a',
      transcriptPath: null,
      eventAt: 1_700_000_000_000,
      subagentsRunning: false,
      informational: false,
    },
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

it('readProgress: reads valid reports and skips invalid files', async () => {
  const at = 1_700_000_000_000
  const write = (name: string, body: string): void => {
    writeAt(join(dirs.progress, name), body, at)
  }
  write('a.json', '{"etaSeconds":360,"step":{"done":4,"total":7}}')
  write('b.json', '{"etaSeconds":60,"step":{"done":5,"total":4}}')
  write('missing.json', '{"step":{"done":1,"total":2}}')
  write('negative.json', '{"etaSeconds":-5}')
  write('fraction.json', '{"etaSeconds":1.5}')
  write('bad.json', '{not json')
  write('a.b.json', '{"etaSeconds":5}')
  write('notes.txt', '{"etaSeconds":5}')

  expect(await readProgress(dirs)).toEqual(
    new Map([
      ['a', { etaMs: 360_000, step: { done: 4, total: 7 }, reportedAt: at }],
      ['b', { etaMs: 60_000, step: null, reportedAt: at }],
    ]),
  )
})

it('readProgress: a missing directory has no reports', async () => {
  expect(await readProgress(dirs)).toEqual(new Map())
})

const activityTranscript = (): { path: string; main: number } => {
  const main = 1_700_000_000_000
  const path = join(root, 'project', 's-1.jsonl')
  writeAt(path, '', main)
  return { path, main }
}

const subagentFile = (
  transcript: string,
  relative: string,
  mtimeMs: number,
): void => {
  writeAt(join(dirname(transcript), 's-1', 'subagents', relative), '', mtimeMs)
}

it('transcriptActivity: is the transcript mtime without subagents', async () => {
  const { path, main } = activityTranscript()
  expect(await transcriptActivity(path, 's-1')).toBe(main)
})

it.each(['agent-x.jsonl', 'workflows/wf_1/agent-y.jsonl'])(
  'transcriptActivity: a newer %s is the activity',
  async (relative) => {
    const { path, main } = activityTranscript()
    subagentFile(path, relative, main + 10_000)
    expect(await transcriptActivity(path, 's-1')).toBe(main + 10_000)
  },
)

it('transcriptActivity: picks the newest of nested subagent transcripts', async () => {
  const { path, main } = activityTranscript()
  subagentFile(path, 'agent-x.jsonl', main + 5_000)
  subagentFile(path, 'workflows/wf_1/agent-y.jsonl', main + 9_000)
  expect(await transcriptActivity(path, 's-1')).toBe(main + 9_000)
})

it.each([
  ['an older subagent transcript', 'agent-x.jsonl', -500_000],
  ['a newer file that is not a transcript', 'agent-x.meta.json', 10_000],
  ['a transcript four parts deep', 'a/b/c/agent-z.jsonl', 10_000],
])('transcriptActivity: ignores %s', async (_label, relative, offset) => {
  const { path, main } = activityTranscript()
  subagentFile(path, relative, main + offset)
  expect(await transcriptActivity(path, 's-1')).toBe(main)
})

it('transcriptActivity: a missing transcript has no activity', async () => {
  expect(
    await transcriptActivity(join(root, 'project', 's-1.jsonl'), 's-1'),
  ).toBeNull()
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
    writeAt(
      join(dirs.progress, `${id}.json`),
      '{"etaSeconds":60}',
      now - age * DAY_MS,
    )
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
  expect(readdirSync(dirs.progress).sort()).toEqual([
    'newDead.json',
    'oldLive.json',
  ])
})

it('pruneStale: does nothing when liveness is unknown', async () => {
  seed()
  await pruneStale(null, now, dirs)

  expect(readdirSync(dirs.status)).toHaveLength(3)
  expect(readdirSync(dirs.seen)).toHaveLength(3)
  expect(readdirSync(dirs.progress)).toHaveLength(3)
})
