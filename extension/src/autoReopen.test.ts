import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  chooseWorkspacePath,
  countActive,
  isActive,
  listReasons,
  reopenAsWorkspace,
  unsafeReasons,
} from './autoReopen'
import type { SessionEvent, SessionStatus } from './sessionState'
import type { WorkspaceDirs } from './workspaceFile'
import { legacyWorkspacePath, siblingWorkspacePath } from './workspaceFile'

let root: string
let dirs: WorkspaceDirs
let folder: string

beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'aydy-reopen-')))
  dirs = { workspaces: join(root, 'support', 'workspaces') }
  folder = join(root, 'okven')
  mkdirSync(folder)
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

const empty = {
  activeSessions: 0,
  busyTerminals: 0,
  dirtyDocuments: 0,
  checkFailed: false,
}

it('unsafeReasons: an empty window is safe to reopen', () => {
  expect(unsafeReasons(empty)).toEqual([])
})

it('unsafeReasons: names each running thing, singular and plural', () => {
  expect(unsafeReasons({ ...empty, activeSessions: 1 })).toEqual([
    'a Claude Code session that is working or waiting on you',
  ])
  expect(unsafeReasons({ ...empty, activeSessions: 2 })).toEqual([
    'Claude Code sessions that are working or waiting on you',
  ])
  expect(unsafeReasons({ ...empty, busyTerminals: 1 })).toEqual([
    'a terminal running a process',
  ])
  expect(unsafeReasons({ ...empty, busyTerminals: 3 })).toEqual([
    'terminals running processes',
  ])
  expect(unsafeReasons({ ...empty, dirtyDocuments: 1 })).toEqual([
    'unsaved files',
  ])
  expect(unsafeReasons({ ...empty, dirtyDocuments: 2 })).toEqual([
    'unsaved files',
  ])
  expect(unsafeReasons({ ...empty, checkFailed: true })).toEqual([
    'processes AYDY could not check',
  ])
})

it('unsafeReasons: lists everything in order', () => {
  expect(
    unsafeReasons({
      activeSessions: 1,
      busyTerminals: 2,
      dirtyDocuments: 3,
      checkFailed: true,
    }),
  ).toEqual([
    'a Claude Code session that is working or waiting on you',
    'terminals running processes',
    'unsaved files',
    'processes AYDY could not check',
  ])
})

const status = (
  sessionId: string,
  event: SessionEvent,
  flags: Partial<
    Pick<SessionStatus, 'subagentsRunning' | 'informational'>
  > = {},
): SessionStatus => ({
  sessionId,
  event,
  cwd: null,
  transcriptPath: null,
  eventAt: 0,
  subagentsRunning: false,
  informational: false,
  ...flags,
})

it('isActive: working and waiting are active, idle is not', () => {
  expect(isActive(status('a', 'UserPromptSubmit'))).toBe(true)
  expect(isActive(status('a', 'Notification'))).toBe(true)
  expect(isActive(status('a', 'Stop', { subagentsRunning: true }))).toBe(true)
  expect(isActive(status('a', 'Stop'))).toBe(false)
  expect(isActive(status('a', 'Notification', { informational: true }))).toBe(
    false,
  )
  expect(isActive(status('a', 'SessionStart'))).toBe(false)
  expect(isActive(undefined)).toBe(false)
})

it('countActive: joins by session id, treating no status as idle', () => {
  expect(
    countActive(
      ['working', 'waiting', 'subagents', 'idle', 'informed', 'new'],
      [
        status('working', 'UserPromptSubmit'),
        status('waiting', 'Notification'),
        status('subagents', 'Stop', { subagentsRunning: true }),
        status('idle', 'Stop'),
        status('informed', 'Notification', { informational: true }),
        status('elsewhere', 'UserPromptSubmit'),
      ],
    ),
  ).toBe(3)
})

it('listReasons: joins as prose', () => {
  expect(listReasons([])).toBe('')
  expect(listReasons(['a'])).toBe('a')
  expect(listReasons(['a', 'b'])).toBe('a and b')
  expect(listReasons(['a', 'b', 'c'])).toBe('a, b and c')
})

const never = (): Promise<boolean> => {
  throw new Error('should not ask')
}

it('chooseWorkspacePath: uses the free sibling without asking', async () => {
  expect(
    await chooseWorkspacePath(folder, { dirs, confirmOverwrite: never }),
  ).toEqual({
    path: siblingWorkspacePath(folder),
    fresh: false,
  })
})

it('chooseWorkspacePath: asks about a foreign sibling, overwriting on yes', async () => {
  const sibling = siblingWorkspacePath(folder)
  writeFileSync(sibling, '{"folders": []}')
  const asked: string[] = []
  const yes = async (path: string): Promise<boolean> => {
    asked.push(path)
    return true
  }
  expect(
    await chooseWorkspacePath(folder, { dirs, confirmOverwrite: yes }),
  ).toEqual({
    path: sibling,
    fresh: true,
  })
  expect(asked).toEqual([sibling])
})

it('chooseWorkspacePath: falls back to the data folder on no', async () => {
  writeFileSync(siblingWorkspacePath(folder), '{"folders": []}')
  expect(
    await chooseWorkspacePath(folder, {
      dirs,
      confirmOverwrite: async () => false,
    }),
  ).toEqual({ path: legacyWorkspacePath(folder, dirs), fresh: false })
})

it('reopenAsWorkspace: writes and registers the file before opening it in the same window', async () => {
  const path = siblingWorkspacePath(folder)
  const calls: [string, { forceReuseWindow: boolean }, boolean][] = []

  await reopenAsWorkspace(folder, '● okven', {
    dirs,
    confirmOverwrite: never,
    openFolder: async (opened, options) => {
      calls.push([opened, options, existsSync(opened)])
    },
  })

  expect(calls).toEqual([[path, { forceReuseWindow: true }, true]])
  const written = JSON.parse(readFileSync(path, 'utf8')) as {
    settings: Record<string, unknown>
  }
  expect(written.settings['window.title']).toBe('● okven')
  expect(existsSync(join(dirs.workspaces, 'index'))).toBe(true)
})
