import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  descendsFrom,
  hasChildren,
  parseParents,
  readLiveSessions,
  sessionIdsUnder,
} from './processTree'

let root: string

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'aydy-tree-'))
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

it('parseParents: reads ps output, skipping junk lines', () => {
  expect(
    parseParents('    1     0\n  412   1\n\n 980  412\nfoo bar\n'),
  ).toEqual(
    new Map([
      [1, 0],
      [412, 1],
      [980, 412],
    ]),
  )
})

it('descendsFrom: follows parents up to the root', () => {
  const parents = new Map([
    [980, 412],
    [412, 300],
    [300, 1],
    [500, 1],
  ])
  expect(descendsFrom(980, 300, parents)).toBe(true)
  expect(descendsFrom(300, 300, parents)).toBe(true)
  expect(descendsFrom(500, 300, parents)).toBe(false)
  expect(descendsFrom(777, 300, parents)).toBe(false)
})

it('descendsFrom: stops on a cycle', () => {
  const parents = new Map([
    [10, 11],
    [11, 10],
  ])
  expect(descendsFrom(10, 300, parents)).toBe(false)
})

it("hasChildren: true only for a pid that is some process's parent", () => {
  const parents = new Map([
    [980, 412],
    [412, 300],
  ])
  expect(hasChildren(412, parents)).toBe(true)
  expect(hasChildren(300, parents)).toBe(true)
  expect(hasChildren(980, parents)).toBe(false)
  expect(hasChildren(777, parents)).toBe(false)
})

it('sessionIdsUnder: keeps sessions descending from the root', () => {
  const parents = new Map([
    [980, 412],
    [412, 300],
    [500, 1],
  ])
  expect(
    sessionIdsUnder(
      [
        { pid: 980, sessionId: 'mine' },
        { pid: 500, sessionId: 'other' },
        { pid: 777, sessionId: 'unknown' },
      ],
      300,
      parents,
    ),
  ).toEqual(['mine'])
})

it('readLiveSessions: keeps live entries and skips dead, broken or unnamed ones', async () => {
  const dir = join(root, 'sessions')
  mkdirSync(dir)
  const live = { pid: process.pid, sessionId: 'abc-123' }
  writeFileSync(join(dir, 'a.json'), JSON.stringify(live))
  writeFileSync(
    join(dir, 'b.json'),
    JSON.stringify({ pid: 2 ** 22 + 12345, sessionId: 'dead' }),
  )
  writeFileSync(join(dir, 'c.json'), '{"pid":')
  writeFileSync(join(dir, 'd.txt'), JSON.stringify(live))
  writeFileSync(join(dir, 'e.json'), JSON.stringify({ pid: process.pid }))
  writeFileSync(
    join(dir, 'f.json'),
    JSON.stringify({ pid: process.pid, sessionId: '../escape' }),
  )
  writeFileSync(
    join(dir, 'g.json'),
    JSON.stringify({ pid: process.pid, sessionId: 42 }),
  )
  expect(await readLiveSessions(dir)).toEqual([live])
  expect(await readLiveSessions(join(root, 'missing'))).toEqual([])
})
