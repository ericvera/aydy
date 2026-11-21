import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import { descendsFrom, parseParents, readSessionPids } from './processTree'

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

it('readSessionPids: keeps live pids and skips dead or broken entries', async () => {
  const dir = join(root, 'sessions')
  mkdirSync(dir)
  writeFileSync(join(dir, 'a.json'), JSON.stringify({ pid: process.pid }))
  writeFileSync(join(dir, 'b.json'), JSON.stringify({ pid: 2 ** 22 + 12345 }))
  writeFileSync(join(dir, 'c.json'), '{"pid":')
  writeFileSync(join(dir, 'd.txt'), JSON.stringify({ pid: process.pid }))
  expect(await readSessionPids(dir)).toEqual([process.pid])
  expect(await readSessionPids(join(root, 'missing'))).toEqual([])
})
