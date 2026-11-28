import { mkdtempSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type {
  SessionEvent,
  SessionStatus,
  WindowFolders,
  WindowGlance,
} from './sessionState'
import type { TrackerDirs, TrackerStore, WatchDir } from './windowTracker'
import { WindowTracker } from './windowTracker'
import type { ProgressReport } from './workSignal'

const DEBOUNCE_MS = 150
const NOW = 1_000_000
const SELF = 100
const OTHER = 200

let root: string
let trackers: WindowTracker[]

beforeEach(() => {
  vi.useFakeTimers({
    toFake: [
      'setTimeout',
      'clearTimeout',
      'setInterval',
      'clearInterval',
      'Date',
    ],
  })
  vi.setSystemTime(NOW)
  trackers = []
})

afterEach(() => {
  for (const tracker of trackers) {
    tracker.dispose()
  }
  vi.useRealTimers()
  if (root !== undefined) {
    rmSync(root, { force: true, recursive: true })
  }
})

const freshDirs = (): TrackerDirs => {
  root = mkdtempSync(join(tmpdir(), 'aydy-tracker-'))
  return {
    status: join(root, 'status'),
    seen: join(root, 'seen'),
    windows: join(root, 'windows'),
    sessions: join(root, 'sessions'),
    progress: join(root, 'progress'),
  }
}

const status = (
  sessionId: string,
  event: SessionEvent,
  cwd: string | null,
  eventAt = NOW - 1000,
  { transcriptPath = null as string | null, subagentsRunning = false } = {},
): SessionStatus => ({
  sessionId,
  event,
  cwd,
  transcriptPath,
  eventAt,
  subagentsRunning,
  informational: false,
})

interface FakeWorld {
  statuses: SessionStatus[]
  seen: Map<string, number>
  live: Map<string, string | null> | null
  others: WindowFolders[]
  writes: string[]
  resolve: (cwd: string | null) => string | null
  progress: Map<string, ProgressReport>
  activity: Map<string, number | Error>
}

const world = (overrides: Partial<FakeWorld> = {}): FakeWorld => ({
  statuses: [],
  seen: new Map(),
  live: new Map(),
  others: [],
  writes: [],
  resolve: (cwd) => cwd,
  progress: new Map(),
  activity: new Map(),
  ...overrides,
})

const storeFor = (w: FakeWorld): TrackerStore => ({
  readStatuses: async () => Promise.resolve([...w.statuses]),
  readSeen: async () => Promise.resolve(new Map(w.seen)),
  readProgress: async () => Promise.resolve(new Map(w.progress)),
  transcriptActivity: async (path) => {
    const activity = w.activity.get(path)
    return activity instanceof Error
      ? Promise.reject(activity)
      : Promise.resolve(activity ?? null)
  },
  writeSeen: async (id, ms) => {
    w.writes.push(id)
    w.seen.set(id, ms)
    return Promise.resolve()
  },
  readLiveSessions: async () =>
    Promise.resolve(w.live === null ? null : new Map(w.live)),
  resolveCwd: async (cwd) => Promise.resolve(w.resolve(cwd)),
  pruneStale: async () => Promise.resolve(),
  knownWindows: async (pid, folders) =>
    Promise.resolve([...w.others, { pid, folders }]),
})

interface Started {
  tracker: WindowTracker
  counts: number[]
  glances: WindowGlance[]
  setFocused: (value: boolean) => void
  dirs: TrackerDirs
}

const realTick = async (): Promise<void> =>
  new Promise<void>((resolve) => {
    setImmediate(resolve)
  })

const settle = async (): Promise<void> => {
  await realTick()
  await vi.advanceTimersByTimeAsync(DEBOUNCE_MS)
  for (let i = 0; i < 5; i += 1) {
    await realTick()
  }
}

// Real watchers on freshly made directories can deliver a late event for
// their creation at any moment, which adds an unplanned pass. Tests drive
// passes with poke() unless they pass a watcher of their own.
const noWatch: WatchDir = () => ({ close: () => undefined })

const startTracker = async (
  w: FakeWorld,
  {
    folders = ['/a'],
    focused = false,
    store = storeFor(w),
    watchDir = noWatch,
  } = {},
): Promise<Started> => {
  let isFocused = focused
  const counts: number[] = []
  const glances: WindowGlance[] = []
  const dirs = freshDirs()
  const tracker = new WindowTracker({
    pid: SELF,
    folders: () => folders,
    focused: () => isFocused,
    onGlance: (glance) => {
      counts.push(glance.unread)
      glances.push(glance)
    },
    dirs,
    store,
    debounceMs: DEBOUNCE_MS,
    watchDir,
  })
  trackers.push(tracker)
  await tracker.start()
  await settle()
  return {
    tracker,
    counts,
    glances,
    dirs,
    setFocused: (value) => {
      isFocused = value
    },
  }
}

const liveIn = (...entries: [string, string | null][]) => new Map(entries)

it('an unfocused window reports the count and writes nothing', async () => {
  const w = world({
    statuses: [
      status('s1', 'Stop', '/a'),
      status('s2', 'Notification', '/a/b'),
    ],
    live: liveIn(['s1', null], ['s2', null]),
  })

  const { counts } = await startTracker(w)

  expect(counts).toEqual([2])
  expect(w.writes).toEqual([])
})

it('a focused window writes seen for owned unread sessions only and reports 0', async () => {
  const w = world({
    statuses: [
      status('s1', 'Stop', '/a'),
      status('s2', 'UserPromptSubmit', '/a'),
      status('s3', 'Stop', '/elsewhere'),
    ],
    live: liveIn(['s1', null], ['s2', null], ['s3', null]),
    others: [{ pid: OTHER, folders: ['/elsewhere'] }],
  })

  const { counts } = await startTracker(w, { focused: true })

  expect(counts).toEqual([0])
  expect(w.writes).toEqual(['s1'])
  expect(w.seen.get('s1')).toBe(NOW + DEBOUNCE_MS)
})

it('nested windows: the parent window neither counts nor clears a worktree session', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a/.claude/worktrees/x/src')],
    live: liveIn(['s1', null]),
    others: [{ pid: OTHER, folders: ['/a/.claude/worktrees/x'] }],
  })

  const { counts } = await startTracker(w, { folders: ['/a'], focused: true })

  expect(counts).toEqual([0])
  expect(w.writes).toEqual([])
})

it('nested windows: the worktree window counts its session and clears it on focus', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a/.claude/worktrees/x/src')],
    live: liveIn(['s1', null]),
    others: [{ pid: OTHER, folders: ['/a'] }],
  })

  const { counts, tracker, setFocused } = await startTracker(w, {
    folders: ['/a/.claude/worktrees/x'],
  })
  expect(counts).toEqual([1])

  setFocused(true)
  tracker.poke()
  await settle()

  expect(counts).toEqual([1, 0])
  expect(w.writes).toEqual(['s1'])
})

it('a Stop arriving while focused stays at 0', async () => {
  const w = world({
    statuses: [status('s1', 'UserPromptSubmit', '/a')],
    live: liveIn(['s1', null]),
  })
  const { glances, tracker } = await startTracker(w, { focused: true })

  w.statuses = [status('s1', 'Stop', '/a', NOW + 10)]
  vi.setSystemTime(NOW + 20)
  tracker.poke()
  await settle()

  expect(glances).toEqual([
    { unread: 0, working: true, signal: { stale: false, text: '<1m' } },
    { unread: 0, working: false, signal: null },
  ])
  expect(w.writes).toEqual(['s1'])
})

it('focus loss followed by a new Stop gives 1', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })
  const { counts, tracker, setFocused } = await startTracker(w, {
    focused: true,
  })

  setFocused(false)
  w.statuses = [status('s1', 'Stop', '/a', NOW + 500)]
  tracker.poke()
  await settle()

  expect(counts).toEqual([0, 1])
})

it('dead sessions are not counted', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a'), status('s2', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })

  const { counts } = await startTracker(w)

  expect(counts).toEqual([1])
})

it('moved session: the launch directory decides ownership over the payload cwd', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a/.claude/worktrees/x')],
    live: liveIn(['s1', '/a']),
    others: [{ pid: OTHER, folders: ['/a/.claude/worktrees/x'] }],
  })

  const { counts, tracker, setFocused } = await startTracker(w, {
    folders: ['/a'],
  })
  expect(counts).toEqual([1])

  setFocused(true)
  tracker.poke()
  await settle()

  expect(counts).toEqual([1, 0])
  expect(w.writes).toEqual(['s1'])
})

it('moved session: the worktree window neither counts nor clears it', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a/.claude/worktrees/x')],
    live: liveIn(['s1', '/a']),
    others: [{ pid: OTHER, folders: ['/a'] }],
  })

  const { counts } = await startTracker(w, {
    folders: ['/a/.claude/worktrees/x'],
    focused: true,
  })

  expect(counts).toEqual([0])
  expect(w.writes).toEqual([])
})

it('fallback: with no live sessions map, the payload cwd decides ownership', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a/x'), status('s2', 'Stop', '/b')],
    live: null,
    others: [{ pid: OTHER, folders: ['/b'] }],
  })

  const { counts } = await startTracker(w)

  expect(counts).toEqual([1])
})

it('fallback: a live session with no launch directory uses the payload cwd', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/b/x')],
    live: liveIn(['s1', null]),
    others: [{ pid: OTHER, folders: ['/b'] }],
  })

  const { counts } = await startTracker(w)

  expect(counts).toEqual([0])
})

it('resolution: resolveCwd is applied before ownership', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/elsewhere')],
    live: liveIn(['s1', '/link']),
    resolve: (cwd) => (cwd === '/link' ? '/a' : cwd),
  })

  const { counts } = await startTracker(w)

  expect(counts).toEqual([1])
})

it('start() creates the four directories inside the given dirs', async () => {
  const { dirs } = await startTracker(world())

  expect(statSync(dirs.status).isDirectory()).toBe(true)
  expect(statSync(dirs.seen).isDirectory()).toBe(true)
  expect(statSync(dirs.windows).isDirectory()).toBe(true)
  expect(statSync(dirs.progress).isDirectory()).toBe(true)
})

it('a window with no folders owns nothing and writes nothing', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })

  const { counts } = await startTracker(w, { folders: [], focused: true })

  expect(counts).toEqual([0])
  expect(w.writes).toEqual([])
})

it('onGlance is not repeated for an unchanged count', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })
  const { counts, tracker } = await startTracker(w)

  tracker.poke()
  await settle()
  tracker.poke()
  await settle()

  expect(counts).toEqual([1])
})

it('a running prompt makes the window working, reported once', async () => {
  const w = world({
    statuses: [status('s1', 'UserPromptSubmit', '/a')],
    live: liveIn(['s1', null]),
  })
  const { glances, tracker } = await startTracker(w)

  tracker.poke()
  await settle()

  expect(glances).toEqual([
    { unread: 0, working: true, signal: { stale: false, text: '<1m' } },
  ])
})

it('a focused pass followed by its own watch event does not write again', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })
  const { tracker } = await startTracker(w, { focused: true })
  expect(w.writes).toEqual(['s1'])

  tracker.poke()
  await settle()

  expect(w.writes).toEqual(['s1'])
})

it('a change in any watched directory runs a pass', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })
  const changes = new Map<string, () => void>()
  let calls = 0
  const base = storeFor(w)
  const store: TrackerStore = {
    ...base,
    readStatuses: async () => {
      calls += 1
      return base.readStatuses()
    },
  }
  const { dirs } = await startTracker(w, {
    store,
    watchDir: (dir, onChange) => {
      changes.set(dir, onChange)
      return { close: () => undefined }
    },
  })
  expect([...changes.keys()]).toEqual([
    dirs.status,
    dirs.seen,
    dirs.windows,
    dirs.progress,
  ])
  expect(calls).toBe(1)

  for (const onChange of changes.values()) {
    onChange()
    await settle()
  }

  expect(calls).toBe(5)
})

it('passes never overlap, and a poke during a pass runs one more', async () => {
  const w = world({
    statuses: [status('s1', 'Stop', '/a')],
    live: liveIn(['s1', null]),
  })
  const base = storeFor(w)
  let active = 0
  let maxActive = 0
  let calls = 0
  let release: (() => void) | null = null
  const store: TrackerStore = {
    ...base,
    readStatuses: async () => {
      calls += 1
      active += 1
      maxActive = Math.max(maxActive, active)
      if (calls === 2) {
        await new Promise<void>((resolve) => {
          release = resolve
        })
      }
      active -= 1
      return base.readStatuses()
    },
  }
  const { tracker } = await startTracker(w, { store })
  expect(calls).toBe(1)

  tracker.poke()
  await settle()
  expect(calls).toBe(2)

  tracker.poke()
  await settle()
  tracker.poke()
  await settle()
  expect(calls).toBe(2)

  const finish = release as (() => void) | null
  finish?.()
  await settle()
  await settle()

  expect(calls).toBe(3)
  expect(maxActive).toBe(1)
})

const MINUTE = 60 * 1000

// Passes run one debounce after the clock is set, so a report stamped at the
// same offset reads with whole minutes left.
const report = (
  etaMinutes: number,
  reportedAt: number,
  step: { done: number; total: number } | null = null,
): ProgressReport => ({
  etaMs: etaMinutes * MINUTE,
  step,
  reportedAt: reportedAt + DEBOUNCE_MS,
})

const working = (id: string, cwd = '/a', eventAt = NOW - 1000) =>
  status(id, 'UserPromptSubmit', cwd, eventAt, {
    transcriptPath: `/t/${id}.jsonl`,
  })

const lastSignal = (glances: WindowGlance[]) => glances.at(-1)?.signal

it('an accepted progress report gives step and ETA', async () => {
  const w = world({
    statuses: [working('s1')],
    live: liveIn(['s1', null]),
    progress: new Map([['s1', report(6, NOW, { done: 4, total: 7 })]]),
    activity: new Map([['/t/s1.jsonl', NOW]]),
  })

  const { glances } = await startTracker(w)

  expect(lastSignal(glances)).toEqual({ stale: false, text: '4/7 · ~6m' })
})

it('a working session quiet for 9 minutes is stale', async () => {
  const w = world({
    statuses: [working('s1', '/a', NOW - 10 * MINUTE)],
    live: liveIn(['s1', null]),
    activity: new Map([['/t/s1.jsonl', NOW + DEBOUNCE_MS - 9 * MINUTE]]),
  })

  const { glances } = await startTracker(w)

  expect(lastSignal(glances)).toEqual({ stale: true, text: 'quiet 9m' })
})

it('the stale session wins over one with an ETA', async () => {
  const w = world({
    statuses: [working('s1', '/a', NOW - 10 * MINUTE), working('s2')],
    live: liveIn(['s1', null], ['s2', null]),
    progress: new Map([['s2', report(6, NOW)]]),
    activity: new Map([
      ['/t/s1.jsonl', NOW - 9 * MINUTE],
      ['/t/s2.jsonl', NOW],
    ]),
  })

  const { glances } = await startTracker(w)

  expect(lastSignal(glances)).toEqual({ stale: true, text: 'quiet 9m' })
})

it('of two ETAs the shorter wins', async () => {
  const w = world({
    statuses: [working('s1'), working('s2')],
    live: liveIn(['s1', null], ['s2', null]),
    progress: new Map([
      ['s1', report(12, NOW)],
      ['s2', report(6, NOW)],
    ]),
    activity: new Map([
      ['/t/s1.jsonl', NOW],
      ['/t/s2.jsonl', NOW],
    ]),
  })

  const { glances } = await startTracker(w)

  expect(lastSignal(glances)).toEqual({ stale: false, text: '~6m' })
})

it('a session owned by another window contributes nothing', async () => {
  const w = world({
    statuses: [working('s1', '/elsewhere', NOW - 10 * MINUTE)],
    live: liveIn(['s1', null]),
    others: [{ pid: OTHER, folders: ['/elsewhere'] }],
    activity: new Map([['/t/s1.jsonl', NOW - 9 * MINUTE]]),
  })

  const { glances } = await startTracker(w)

  expect(glances).toEqual([{ unread: 0, working: false, signal: null }])
})

it('onGlance repeats only when the signal text changes', async () => {
  const w = world({
    statuses: [working('s1')],
    live: liveIn(['s1', null]),
    progress: new Map([['s1', report(6.5, NOW)]]),
    activity: new Map([['/t/s1.jsonl', NOW]]),
  })
  const { glances, tracker } = await startTracker(w)

  vi.setSystemTime(NOW + 10 * 1000)
  tracker.poke()
  await settle()
  expect(glances).toHaveLength(1)

  w.progress = new Map([['s1', report(3, NOW)]])
  tracker.poke()
  await settle()

  expect(glances.map((glance) => glance.signal?.text)).toEqual(['~6m', '~2m'])
})

it('a transcript read that throws costs only its own session', async () => {
  const w = world({
    statuses: [working('s1'), working('s2', '/a', NOW - 10 * MINUTE)],
    live: liveIn(['s1', null], ['s2', null]),
    progress: new Map([['s1', report(6, NOW)]]),
    activity: new Map<string, number | Error>([
      ['/t/s1.jsonl', new Error('EACCES')],
      ['/t/s2.jsonl', NOW - 9 * MINUTE],
    ]),
  })

  const { glances } = await startTracker(w)

  expect(lastSignal(glances)).toEqual({ stale: true, text: 'quiet 9m' })
})

const stopWithSubagents = (eventAt: number) =>
  status('s1', 'Stop', '/a', eventAt, {
    transcriptPath: '/t/s1.jsonl',
    subagentsRunning: true,
  })

it('the turn start carries across passes', async () => {
  const t0 = NOW
  const w = world({
    statuses: [working('s1', '/a', t0)],
    live: liveIn(['s1', null]),
    activity: new Map([['/t/s1.jsonl', t0]]),
  })
  const { glances, tracker } = await startTracker(w)

  vi.setSystemTime(t0 + 5 * MINUTE)
  w.statuses = [stopWithSubagents(t0 + 5 * MINUTE)]
  w.activity = new Map([['/t/s1.jsonl', t0 + 5 * MINUTE]])
  w.progress = new Map([
    ['s1', report(6, t0 + 2 * MINUTE, { done: 4, total: 7 })],
  ])
  tracker.poke()
  await settle()

  expect(lastSignal(glances)).toEqual({ stale: false, text: '4/7 · ~3m' })
})

it('a report written before the turn started is ignored', async () => {
  const t0 = NOW
  const w = world({
    statuses: [working('s1', '/a', t0)],
    live: liveIn(['s1', null]),
    activity: new Map([['/t/s1.jsonl', t0]]),
  })
  const { glances, tracker } = await startTracker(w)

  vi.setSystemTime(t0 + 5 * MINUTE)
  w.statuses = [stopWithSubagents(t0 + 5 * MINUTE)]
  w.activity = new Map([['/t/s1.jsonl', t0 + 5 * MINUTE]])
  w.progress = new Map([['s1', report(6, t0 - MINUTE, { done: 4, total: 7 })]])
  tracker.poke()
  await settle()

  expect(lastSignal(glances)).toEqual({ stale: false, text: '5m' })
})

it('a plain Stop ends the turn, so the next prompt starts afresh', async () => {
  const t0 = NOW
  const w = world({
    statuses: [working('s1', '/a', t0)],
    live: liveIn(['s1', null]),
    activity: new Map([['/t/s1.jsonl', t0]]),
  })
  const { glances, tracker } = await startTracker(w)

  vi.setSystemTime(t0 + 5 * MINUTE)
  w.statuses = [status('s1', 'Stop', '/a', t0 + 5 * MINUTE)]
  tracker.poke()
  await settle()
  expect(lastSignal(glances)).toBeNull()

  const t1 = t0 + 8 * MINUTE
  vi.setSystemTime(t1)
  w.statuses = [working('s1', '/a', t1)]
  w.activity = new Map([['/t/s1.jsonl', t1]])
  w.progress = new Map([['s1', report(6, t1 - MINUTE, { done: 4, total: 7 })]])
  tracker.poke()
  await settle()

  expect(lastSignal(glances)).toEqual({ stale: false, text: '<1m' })
})
