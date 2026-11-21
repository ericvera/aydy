import { existsSync, readFileSync, realpathSync, writeFileSync } from 'node:fs'
import { mkdir, mkdtemp, rm, symlink } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type { JumpRequest, JumpResponderOptions } from './jumpRequests'
import {
  JumpResponder,
  parseJumpRequest,
  REPLY_BUDGET_MS,
  shouldHandle,
} from './jumpRequests'
import type { RevealOutcome, RevealRequest } from './revealSession'
import type { WindowFolders } from './sessionState'

const NOW = 1_000_000
const PARENT = '/a'
const WORKTREE = '/a/.claude/worktrees/x'
const NESTED: WindowFolders[] = [
  { pid: 1, folders: [PARENT] },
  { pid: 2, folders: [WORKTREE] },
]

let roots: string[]
let responders: JumpResponder[]

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
  roots = []
  responders = []
})

afterEach(async () => {
  for (const responder of responders) {
    responder.dispose()
  }
  vi.useRealTimers()
  for (const root of roots) {
    await rm(root, { force: true, recursive: true })
  }
})

const tempDir = async (): Promise<string> => {
  const root = await mkdtemp(join(tmpdir(), 'aydy-jump-'))
  roots.push(root)
  return root
}

const request = (overrides: Partial<JumpRequest> = {}): JumpRequest => ({
  id: 'jump-1',
  sessionId: 'session-1',
  cwd: '/a/src',
  tabLabel: 'Fix the auth flow',
  requestedAt: NOW,
  ...overrides,
})

const writeRequest = (dir: string, value: JumpRequest): void => {
  writeFileSync(join(dir, `${value.id}.json`), JSON.stringify(value))
}

const requestPath = (dir: string, id = 'jump-1'): string =>
  join(dir, `${id}.json`)

const replyPath = (dir: string, id = 'jump-1'): string =>
  join(dir, `${id}.reply.json`)

const readReply = (dir: string, id = 'jump-1'): unknown =>
  JSON.parse(readFileSync(replyPath(dir, id), 'utf8'))

type Overrides = Partial<JumpResponderOptions> & { dir: string; pid: number }

const responder = (
  options: Overrides,
): {
  responder: JumpResponder
  reveal: ReturnType<typeof vi.fn<(r: RevealRequest) => Promise<RevealOutcome>>>
} => {
  const reveal = vi.fn<(r: RevealRequest) => Promise<RevealOutcome>>(() =>
    Promise.resolve('revealed'),
  )
  const folders = NESTED.find(({ pid }) => pid === options.pid)?.folders ?? [
    PARENT,
  ]
  const created = new JumpResponder({
    folders: () => folders,
    windows: () => Promise.resolve(NESTED),
    launchCwds: () => Promise.resolve(new Map()),
    open: () => null,
    reveal,
    now: () => Date.now(),
    ...options,
  })
  responders.push(created)
  return { responder: created, reveal }
}

it('accepts a well-formed request', () => {
  expect(parseJumpRequest(JSON.stringify(request()))).toEqual(request())
  expect(parseJumpRequest(JSON.stringify(request({ tabLabel: null })))).toEqual(
    request({ tabLabel: null }),
  )
})

it.each([
  ['not json', '{'],
  ['an array', '[]'],
  ['null', 'null'],
  ['a missing id', JSON.stringify({ ...request(), id: undefined })],
  ['a numeric id', JSON.stringify({ ...request(), id: 3 })],
  ['a path in the id', JSON.stringify(request({ id: '../x' }))],
  ['an empty id', JSON.stringify(request({ id: '' }))],
  ['a long id', JSON.stringify(request({ id: 'a'.repeat(129) }))],
  ['a numeric session', JSON.stringify({ ...request(), sessionId: 1 })],
  ['a null cwd', JSON.stringify({ ...request(), cwd: null })],
  ['a missing label', JSON.stringify({ ...request(), tabLabel: undefined })],
  ['a numeric label', JSON.stringify({ ...request(), tabLabel: 1 })],
  ['a string time', JSON.stringify({ ...request(), requestedAt: '1' })],
])('rejects %s', (_, text) => {
  expect(parseJumpRequest(text)).toBeNull()
})

it('handles only requests this window owns', () => {
  const inWorktree = request({ cwd: `${WORKTREE}/src` })
  expect(shouldHandle(inWorktree, 2, NESTED, NOW)).toBe(true)
  expect(shouldHandle(inWorktree, 1, NESTED, NOW)).toBe(false)
  expect(shouldHandle(request(), 1, NESTED, NOW)).toBe(true)
  expect(shouldHandle(request({ cwd: '/b' }), 1, NESTED, NOW)).toBe(false)
})

it('ignores requests older than 10 seconds', () => {
  expect(shouldHandle(request(), 1, NESTED, NOW + 10_000)).toBe(true)
  expect(shouldHandle(request(), 1, NESTED, NOW + 10_001)).toBe(false)
})

it('claims an owned request, reveals it and replies with open', async () => {
  const dir = await tempDir()
  writeRequest(dir, request())
  const { responder: self, reveal } = responder({
    dir,
    pid: 1,
    open: () => '/ws/a.code-workspace',
  })

  await self.start()

  expect(reveal).toHaveBeenCalledWith({
    sessionId: 'session-1',
    tabLabel: 'Fix the auth flow',
  })
  expect(existsSync(requestPath(dir))).toBe(false)
  expect(readReply(dir)).toEqual({
    open: '/ws/a.code-workspace',
    reveal: 'revealed',
  })
})

it('falls back to the owning folder when there is no workspace file', async () => {
  const dir = await tempDir()
  writeRequest(dir, request())
  const { responder: self } = responder({ dir, pid: 1 })

  await self.scan()

  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})

it('replies notPresent without revealing when there is no label', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ tabLabel: null }))
  const { responder: self, reveal } = responder({ dir, pid: 1 })

  await self.scan()

  expect(reveal).not.toHaveBeenCalled()
  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'notPresent' })
})

it('leaves a foreign request untouched', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ cwd: '/elsewhere' }))
  const { responder: self, reveal } = responder({ dir, pid: 1 })

  await self.scan()

  expect(reveal).not.toHaveBeenCalled()
  expect(existsSync(requestPath(dir))).toBe(true)
  expect(existsSync(replyPath(dir))).toBe(false)
})

it('leaves a stale request in place', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ requestedAt: NOW - 10_001 }))
  const { responder: self, reveal } = responder({ dir, pid: 1 })

  await self.scan()

  expect(reveal).not.toHaveBeenCalled()
  expect(existsSync(requestPath(dir))).toBe(true)
})

it('hands a worktree session to the worktree window only', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ cwd: `${WORKTREE}/src` }))
  const parent = responder({ dir, pid: 1 })
  const worktree = responder({ dir, pid: 2 })

  await parent.responder.scan()

  expect(parent.reveal).not.toHaveBeenCalled()
  expect(existsSync(requestPath(dir))).toBe(true)

  await worktree.responder.scan()

  expect(worktree.reveal).toHaveBeenCalledTimes(1)
  expect(readReply(dir)).toEqual({ open: WORKTREE, reveal: 'revealed' })
})

it('hands a parent session to the parent window', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ cwd: '/a/src' }))
  const parent = responder({ dir, pid: 1 })
  const worktree = responder({ dir, pid: 2 })

  await worktree.responder.scan()
  await parent.responder.scan()

  expect(worktree.reveal).not.toHaveBeenCalled()
  expect(parent.reveal).toHaveBeenCalledTimes(1)
  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})

it('decides a moved session by its launch directory', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ cwd: `${WORKTREE}/src` }))
  const launchCwds = (): Promise<Map<string, string | null>> =>
    Promise.resolve(new Map([['session-1', PARENT]]))
  const parent = responder({ dir, pid: 1, launchCwds })
  const worktree = responder({ dir, pid: 2, launchCwds })

  await worktree.responder.scan()
  await parent.responder.scan()

  expect(worktree.reveal).not.toHaveBeenCalled()
  expect(parent.reveal).toHaveBeenCalledTimes(1)
  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})

it.each([
  ['unreadable', null],
  ['without the session', new Map([['other', PARENT]])],
  ['with no launch directory', new Map([['session-1', null]])],
])('lets the request cwd decide when live sessions are %s', async (_, live) => {
  const dir = await tempDir()
  writeRequest(dir, request({ cwd: `${WORKTREE}/src` }))
  const launchCwds = (): Promise<ReadonlyMap<string, string | null> | null> =>
    Promise.resolve(live)
  const parent = responder({ dir, pid: 1, launchCwds })
  const worktree = responder({ dir, pid: 2, launchCwds })

  await parent.responder.scan()
  await worktree.responder.scan()

  expect(parent.reveal).not.toHaveBeenCalled()
  expect(worktree.reveal).toHaveBeenCalledTimes(1)
})

it('resolves the session directory before deciding ownership', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ cwd: '/link/src' }))
  const { responder: self, reveal } = responder({
    dir,
    pid: 1,
    resolveCwd: (cwd) => Promise.resolve(cwd === '/link/src' ? '/a/src' : cwd),
  })

  await self.scan()

  expect(reveal).toHaveBeenCalledTimes(1)
  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})

it('realpaths a symlinked request cwd by default', async () => {
  const dir = await tempDir()
  const root = await tempDir()
  const real = join(root, 'real')
  const link = join(root, 'link')
  await mkdir(join(real, 'src'), { recursive: true })
  await symlink(real, link)
  const realFolder = realpathSync(real)
  writeRequest(dir, request({ cwd: join(link, 'src') }))
  const { responder: self, reveal } = responder({
    dir,
    pid: 7,
    folders: () => [realFolder],
    windows: () => Promise.resolve([{ pid: 7, folders: [realFolder] }]),
  })

  await self.scan()

  expect(reveal).toHaveBeenCalledTimes(1)
  expect(readReply(dir)).toEqual({ open: realFolder, reveal: 'revealed' })
})

it('skips the reply when the reveal runs past the budget', async () => {
  const dir = await tempDir()
  writeRequest(dir, request())
  const { responder: self } = responder({
    dir,
    pid: 1,
    reveal: () => {
      vi.setSystemTime(NOW + REPLY_BUDGET_MS + 1)
      return Promise.resolve('revealed')
    },
  })

  await self.scan()

  expect(existsSync(requestPath(dir))).toBe(false)
  expect(existsSync(replyPath(dir))).toBe(false)
})

it('replies when the reveal finishes within the budget', async () => {
  const dir = await tempDir()
  writeRequest(dir, request())
  const { responder: self } = responder({
    dir,
    pid: 1,
    reveal: () => {
      vi.setSystemTime(NOW + REPLY_BUDGET_MS)
      return Promise.resolve('revealed')
    },
  })

  await self.scan()

  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})

it('lets only one of two self-declared owners reply', async () => {
  const dir = await tempDir()
  writeRequest(dir, request())
  const reveals: number[] = []
  const make = (pid: number): JumpResponder =>
    responder({
      dir,
      pid,
      folders: () => [PARENT],
      windows: () => Promise.resolve([{ pid, folders: [PARENT] }]),
      reveal: () => {
        reveals.push(pid)
        return Promise.resolve('revealed')
      },
    }).responder

  await Promise.all([make(1).scan(), make(2).scan()])

  expect(reveals).toHaveLength(1)
  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})

it('handles each request once', async () => {
  const dir = await tempDir()
  writeRequest(dir, request({ requestedAt: NOW - REPLY_BUDGET_MS - 1 }))
  const { responder: self, reveal } = responder({ dir, pid: 1 })

  await self.scan()
  writeRequest(dir, request({ requestedAt: NOW - REPLY_BUDGET_MS - 1 }))
  await self.scan()

  expect(reveal).toHaveBeenCalledTimes(1)
  expect(existsSync(requestPath(dir))).toBe(true)
})

it('picks up a request written after it started', async () => {
  const dir = await tempDir()
  const { responder: self, reveal } = responder({ dir, pid: 1, pollMs: 50 })
  await self.start()

  writeRequest(dir, request())
  await vi.advanceTimersByTimeAsync(50)
  await self.scan()

  expect(reveal).toHaveBeenCalledTimes(1)
  expect(readReply(dir)).toEqual({ open: PARENT, reveal: 'revealed' })
})
