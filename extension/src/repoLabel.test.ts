import {
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import type { RepoIdentity } from './repoLabel'
import { folderLabel, identifyRepo, readBranch, repoLabel } from './repoLabel'

let dir: string

beforeEach(() => {
  dir = realpathSync(mkdtempSync(join(tmpdir(), 'aydy-repo-')))
})

afterEach(() => {
  rmSync(dir, { force: true, recursive: true })
})

const checkout = (name: string, branch = 'main'): string => {
  const root = join(dir, name)
  mkdirSync(join(root, '.git'), { recursive: true })
  writeFileSync(join(root, '.git', 'HEAD'), `ref: refs/heads/${branch}\n`)
  return root
}

const worktree = (parent: string, root: string, name: string): string => {
  const linked = join(parent, name)
  const gitDir = join(root, '.git', 'worktrees', name)
  mkdirSync(linked, { recursive: true })
  mkdirSync(gitDir, { recursive: true })
  writeFileSync(join(gitDir, 'HEAD'), 'ref: refs/heads/feat/x\n')
  writeFileSync(join(linked, '.git'), `gitdir: ${gitDir}\n`)
  return linked
}

const pointerAt = (path: string, contents: string): string => {
  mkdirSync(path, { recursive: true })
  writeFileSync(join(path, '.git'), contents)
  return path
}

it('a linked worktree belongs to the repository it points at', async () => {
  const root = checkout('okven')
  const linked = worktree(
    join(dir, 'okven.worktrees'),
    root,
    'eric-attribution-2',
  )

  const identity = await identifyRepo(linked)

  expect(identity).toMatchObject({
    name: 'okven',
    root,
    worktree: 'eric-attribution-2',
    gitDir: join(root, '.git', 'worktrees', 'eric-attribution-2'),
  })
})

it('a subdirectory of a linked worktree still resolves to the repository', async () => {
  const root = checkout('okven')
  const linked = worktree(
    join(dir, 'okven.worktrees'),
    root,
    'eric-attribution-2',
  )
  const inside = join(linked, 'functions', 'src')
  mkdirSync(inside, { recursive: true })

  const identity = await identifyRepo(inside)

  expect(identity?.name).toBe('okven')
  expect(identity?.worktree).toBe('eric-attribution-2')
})

it('a normal checkout takes the checkout’s name', async () => {
  const root = checkout('aydy')

  expect(await identifyRepo(root)).toEqual({
    root,
    name: 'aydy',
    worktree: null,
    gitDir: join(root, '.git'),
  })
})

it('a subdirectory of a checkout still finds the root', async () => {
  const root = checkout('aydy')
  const inside = join(root, 'src-tauri', 'src', 'discovery')
  mkdirSync(inside, { recursive: true })

  const identity = await identifyRepo(inside)

  expect(identity?.name).toBe('aydy')
  expect(identity?.root).toBe(root)
})

it('a folder outside any repository has no repository', async () => {
  expect(await identifyRepo(dir)).toBeNull()
})

it('a folder that does not exist has no repository', async () => {
  expect(await identifyRepo(join(dir, 'gone', 'was-here'))).toBeNull()
})

it('a .git file carrying no pointer has no repository', async () => {
  const workingTree = pointerAt(join(dir, 'mystery'), 'no pointer here\n')

  expect(await identifyRepo(workingTree)).toBeNull()
})

it('a pointer that is not a worktrees pointer names the working tree', async () => {
  const workingTree = pointerAt(
    join(dir, 'shared-types'),
    'gitdir: /Users/eric/Code/okven/.git/modules/shared-types\n',
  )

  const identity = await identifyRepo(workingTree)

  expect(identity?.name).toBe('shared-types')
  expect(identity?.worktree).toBeNull()
})

it('a relative pointer resolves against the worktree', async () => {
  const root = checkout('okven')
  const linked = pointerAt(
    join(dir, 'okven.worktrees', 'eric-fab-snippet'),
    'gitdir: ../../okven/.git/worktrees/eric-fab-snippet\n',
  )

  const identity = await identifyRepo(linked)

  expect(identity?.name).toBe('okven')
  expect(identity?.root).toBe(root)
  expect(identity?.worktree).toBe('eric-fab-snippet')
})

it('a worktree of a bare repository strips the .git suffix', async () => {
  const linked = pointerAt(
    join(dir, 'checkouts', 'eric-fix-logo-size'),
    `gitdir: ${dir}/okven.git/worktrees/eric-fix-logo-size\n`,
  )

  const identity = await identifyRepo(linked)

  expect(identity?.name).toBe('okven')
  expect(identity?.root).toBe(join(dir, 'okven.git'))
})

it.each([
  ['ref: refs/heads/main\n', 'main'],
  ['ref: refs/heads/feat/tab-state\n', 'feat/tab-state'],
  ['3d300eb1c2a4f5e6d7c8b9a0e1f2d3c4b5a6e7f8\n', null],
])('readBranch(%j)', async (head, branch) => {
  writeFileSync(join(dir, 'HEAD'), head)

  expect(await readBranch(dir)).toBe(branch)
})

it('readBranch with no HEAD is null', async () => {
  expect(await readBranch(dir)).toBeNull()
})

const repo = (worktree: string | null): RepoIdentity => ({
  root: '/code/aydy',
  name: 'aydy',
  worktree,
  gitDir: '/code/aydy/.git',
})

it.each([
  [repo('feat-tab-state'), 'feat/x', 'aydy · feat-tab-state'],
  [repo(null), 'feat/tab-state', 'aydy · feat/tab-state'],
  [repo(null), 'main', 'aydy'],
  [repo(null), 'master', 'aydy'],
  [repo(null), null, 'aydy'],
  [null, null, 'plain'],
])('repoLabel(%j, %j)', (identity, branch, label) => {
  expect(repoLabel(identity, branch, '/code/plain')).toBe(label)
})

it('folderLabel watches HEAD for a checkout', async () => {
  const root = checkout('aydy', 'feat/tab-state')

  expect(await folderLabel(root)).toEqual({
    label: 'aydy · feat/tab-state',
    headPath: join(root, '.git', 'HEAD'),
  })
})

it('folderLabel watches nothing for a worktree', async () => {
  const root = checkout('okven')
  const linked = worktree(
    join(dir, 'okven.worktrees'),
    root,
    'eric-attribution-2',
  )

  expect(await folderLabel(linked)).toEqual({
    label: 'okven · eric-attribution-2',
    headPath: null,
  })
})

it('folderLabel outside git is the folder name', async () => {
  const plain = join(dir, 'plain')
  mkdirSync(plain)

  expect(await folderLabel(plain)).toEqual({ label: 'plain', headPath: null })
})
