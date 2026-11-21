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
  listReasons,
  reopenAsWorkspace,
  unsafeReasons,
} from './autoReopen'
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
  terminals: 0,
  claudeTabs: 0,
  claudeSessions: 0,
  dirtyDocuments: 0,
}

it('unsafeReasons: an empty window is safe to reopen', () => {
  expect(unsafeReasons(empty)).toEqual([])
})

it('unsafeReasons: names what reopening would end', () => {
  expect(unsafeReasons({ ...empty, claudeTabs: 1 })).toEqual([
    'Claude Code sessions',
  ])
  expect(unsafeReasons({ ...empty, claudeSessions: 2 })).toEqual([
    'Claude Code sessions',
  ])
  expect(unsafeReasons({ ...empty, terminals: 1 })).toEqual(['a terminal'])
  expect(
    unsafeReasons({
      terminals: 3,
      claudeTabs: 0,
      claudeSessions: 1,
      dirtyDocuments: 2,
    }),
  ).toEqual(['Claude Code sessions', 'terminals', 'unsaved files'])
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
