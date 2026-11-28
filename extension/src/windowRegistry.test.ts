import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  knownWindows,
  publishWindow,
  readWindows,
  unpublishWindow,
} from './windowRegistry'

let root: string
let dir: string
const dead = new Set<number>()
const isAlive = (pid: number): boolean => !dead.has(pid)

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'aydy-windows-'))
  dir = join(root, 'windows')
  dead.clear()
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

it('publish then read round-trips', async () => {
  await publishWindow(101, ['/a', '/b'], null, { dir })
  await publishWindow(102, [], null, { dir })

  const windows = await readWindows({ dir, isAlive })
  expect(windows.sort((x, y) => x.pid - y.pid)).toEqual([
    { pid: 101, folders: ['/a', '/b'], workspace: null },
    { pid: 102, folders: [], workspace: null },
  ])
})

it('leaves out and deletes a dead window', async () => {
  await publishWindow(101, ['/a'], null, { dir })
  await publishWindow(202, ['/b'], null, { dir })
  dead.add(202)

  expect(await readWindows({ dir, isAlive })).toEqual([
    { pid: 101, folders: ['/a'], workspace: null },
  ])
  expect(existsSync(join(dir, '202.json'))).toBe(false)
})

it('skips malformed files and mismatched pids', async () => {
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, '1.json'), '{')
  writeFileSync(
    join(dir, '2.json'),
    JSON.stringify({ pid: 3, folders: ['/a'] }),
  )
  writeFileSync(join(dir, '4.json'), JSON.stringify({ pid: 4, folders: [1] }))
  writeFileSync(join(dir, '5.json'), JSON.stringify({ pid: 5 }))
  writeFileSync(
    join(dir, '.6.json.1.abc.tmp'),
    JSON.stringify({ pid: 6, folders: [] }),
  )
  writeFileSync(join(dir, 'x.json'), JSON.stringify({ pid: 7, folders: [] }))
  writeFileSync(
    join(dir, '8.json'),
    JSON.stringify({ pid: 8, folders: ['/ok'] }),
  )

  expect(await readWindows({ dir, isAlive })).toEqual([
    { pid: 8, folders: ['/ok'], workspace: null },
  ])
})

it('returns no windows for a missing dir', async () => {
  expect(await readWindows({ dir, isAlive })).toEqual([])
})

it('unpublishWindow removes the file and ignores a missing one', async () => {
  await publishWindow(101, ['/a'], null, { dir })
  await unpublishWindow(101, { dir })
  await unpublishWindow(101, { dir })

  expect(readdirSync(dir)).toEqual([])
})

it('knownWindows replaces a stale self entry with the in-memory folders', async () => {
  await publishWindow(101, ['/old'], null, { dir })
  await publishWindow(202, ['/b'], null, { dir })

  const windows = await knownWindows(101, ['/new'], { dir, isAlive })
  expect(windows.sort((x, y) => x.pid - y.pid)).toEqual([
    { pid: 101, folders: ['/new'] },
    { pid: 202, folders: ['/b'], workspace: null },
  ])
})

it('publishWindow writes the open workspace and readWindows reads it back', async () => {
  await publishWindow(101, ['/a'], '/a.code-workspace', { dir })

  expect(
    JSON.parse(readFileSync(join(dir, '101.json'), 'utf8')) as unknown,
  ).toEqual({ pid: 101, folders: ['/a'], workspace: '/a.code-workspace' })
  expect(await readWindows({ dir, isAlive })).toEqual([
    { pid: 101, folders: ['/a'], workspace: '/a.code-workspace' },
  ])
})

it('reads a window file without a workspace as workspace null', async () => {
  mkdirSync(dir, { recursive: true })
  writeFileSync(
    join(dir, '9.json'),
    JSON.stringify({ pid: 9, folders: ['/a'] }),
  )

  expect(await readWindows({ dir, isAlive })).toEqual([
    { pid: 9, folders: ['/a'], workspace: null },
  ])
})

it('keeps a window whose workspace is not a string and reads it as null', async () => {
  mkdirSync(dir, { recursive: true })
  writeFileSync(
    join(dir, '9.json'),
    JSON.stringify({ pid: 9, folders: ['/a'], workspace: 42 }),
  )

  expect(await readWindows({ dir, isAlive })).toEqual([
    { pid: 9, folders: ['/a'], workspace: null },
  ])
})
