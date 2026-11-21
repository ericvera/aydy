import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import { offerKey, reopenAsWorkspace, shouldOffer } from './reopenOffer'
import type { WorkspaceDirs } from './workspaceFile'
import { generatedWorkspacePath } from './workspaceFile'

let root: string
let dirs: WorkspaceDirs
let folder: string

beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'aydy-reopen-')))
  dirs = { workspaces: join(root, 'workspaces') }
  folder = join(root, 'okven')
  mkdirSync(folder)
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

it('keys the offer by folder', () => {
  expect(offerKey('/Users/me/okven')).toBe('aydy.reopenOffered:/Users/me/okven')
})

it('offers for a single folder with no workspace file, not yet offered', () => {
  expect(shouldOffer({ folders: ['/a'], offered: false })).toBe(true)
})

it.each([
  ['multi-root', { folders: ['/a', '/b'], offered: false }],
  [
    'workspace file present',
    { workspaceFile: '/w.code-workspace', folders: ['/a'], offered: false },
  ],
  ['already offered', { folders: ['/a'], offered: true }],
  ['no folders', { folders: [], offered: false }],
])('does not offer when %s', (_, state) => {
  expect(shouldOffer(state)).toBe(false)
})

it('writes the workspace file before opening it in the same window', async () => {
  const path = generatedWorkspacePath(folder, dirs)
  const calls: [string, { forceReuseWindow: boolean }, boolean][] = []

  await reopenAsWorkspace(folder, '● okven', {
    dirs,
    openFolder: async (opened, options) => {
      calls.push([opened, options, existsSync(opened)])
    },
  })

  expect(path.startsWith(dirs.workspaces)).toBe(true)
  expect(calls).toEqual([[path, { forceReuseWindow: true }, true]])
  const written = JSON.parse(readFileSync(path, 'utf8')) as {
    settings: Record<string, unknown>
  }
  expect(written.settings['window.title']).toBe('● okven')
})
