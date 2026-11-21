import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  ChangedOnDisk,
  resolveTarget,
  writeAtomically,
  writeIfUnchanged,
} from './atomicWrite'

let root: string
let previousUmask: number

beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'aydy-atomic-')))
  previousUmask = process.umask(0o022)
})

afterEach(() => {
  process.umask(previousUmask)
  rmSync(root, { force: true, recursive: true })
})

const modeOf = (path: string): number => statSync(path).mode & 0o777

const leftovers = (directory: string): string[] =>
  readdirSync(directory).filter((name) => name.endsWith('.tmp'))

it('writeAtomically: writes the text and applies an explicit mode', async () => {
  const target = join(root, 'script.sh')
  await writeAtomically(target, 'echo hi\n', 0o755)
  expect(readFileSync(target, 'utf8')).toBe('echo hi\n')
  expect(modeOf(target)).toBe(0o755)
  expect(leftovers(root)).toEqual([])
})

it('writeAtomically: keeps an existing 0600 target private', async () => {
  const target = join(root, 'settings.json')
  writeFileSync(target, '{}', { mode: 0o600 })
  await writeAtomically(target, '{"a":1}')
  expect(readFileSync(target, 'utf8')).toBe('{"a":1}')
  expect(modeOf(target)).toBe(0o600)
  expect(leftovers(root)).toEqual([])
})

it('writeAtomically: creates missing parent directories', async () => {
  const target = join(root, 'a', 'b', 'settings.json')
  await writeAtomically(target, '{}')
  expect(readFileSync(target, 'utf8')).toBe('{}')
  expect(leftovers(join(root, 'a', 'b'))).toEqual([])
})

it('writeAtomically: writes through a symlink and keeps the link', async () => {
  const real = join(root, 'dotfiles', 'settings.json')
  mkdirSync(join(root, 'dotfiles'))
  writeFileSync(real, 'old', { mode: 0o600 })
  const link = join(root, 'settings.json')
  symlinkSync(real, link)
  await writeAtomically(link, 'new')
  expect(lstatSync(link).isSymbolicLink()).toBe(true)
  expect(readlinkSync(link)).toBe(real)
  expect(readFileSync(real, 'utf8')).toBe('new')
  expect(modeOf(real)).toBe(0o600)
  expect(leftovers(root)).toEqual([])
  expect(leftovers(join(root, 'dotfiles'))).toEqual([])
})

it('writeAtomically: creates the missing target of a dangling symlink and its directory', async () => {
  const real = join(root, 'missing', 'dir', 'settings.json')
  const link = join(root, 'settings.json')
  symlinkSync(real, link)
  expect(await resolveTarget(link)).toBe(real)
  await writeAtomically(link, 'text')
  expect(lstatSync(link).isSymbolicLink()).toBe(true)
  expect(readlinkSync(link)).toBe(real)
  expect(readFileSync(real, 'utf8')).toBe('text')
  expect(leftovers(root)).toEqual([])
  expect(leftovers(join(root, 'missing', 'dir'))).toEqual([])
})

it('writeAtomically: follows a dangling chain one hop at a time', async () => {
  const real = join(root, 'final.json')
  symlinkSync('final.json', join(root, 'middle'))
  symlinkSync('middle', join(root, 'start'))
  expect(await resolveTarget(join(root, 'start'))).toBe(real)
})

it('writeAtomically: gives up on a symlink loop', async () => {
  symlinkSync('b', join(root, 'a'))
  symlinkSync('a', join(root, 'b'))
  await expect(resolveTarget(join(root, 'a'))).rejects.toThrow()
})

it('writeIfUnchanged: writes when the content matches and keeps a 0600 target private', async () => {
  const target = join(root, 'settings.json')
  writeFileSync(target, 'old', { mode: 0o600 })
  await writeIfUnchanged(target, 'old', 'new')
  expect(readFileSync(target, 'utf8')).toBe('new')
  expect(modeOf(target)).toBe(0o600)
  expect(leftovers(root)).toEqual([])
})

it('writeIfUnchanged: aborts with ChangedOnDisk and leaves the file intact', async () => {
  const target = join(root, 'settings.json')
  writeFileSync(target, 'theirs')
  await expect(writeIfUnchanged(target, 'mine', 'new')).rejects.toThrow(
    ChangedOnDisk,
  )
  expect(readFileSync(target, 'utf8')).toBe('theirs')
  expect(leftovers(root)).toEqual([])
})

it('writeIfUnchanged: aborts when a file appeared that was expected to be missing', async () => {
  const target = join(root, 'settings.json')
  writeFileSync(target, 'theirs')
  await expect(writeIfUnchanged(target, null, 'new')).rejects.toThrow(
    ChangedOnDisk,
  )
  expect(readFileSync(target, 'utf8')).toBe('theirs')
  expect(leftovers(root)).toEqual([])
})

it('writeIfUnchanged: creates missing parent directories when the file was expected missing', async () => {
  const target = join(root, 'a', 'b', 'settings.json')
  await writeIfUnchanged(target, null, '{}')
  expect(readFileSync(target, 'utf8')).toBe('{}')
  expect(leftovers(join(root, 'a', 'b'))).toEqual([])
})

it('writeIfUnchanged: writes through a symlink and keeps the link', async () => {
  const real = join(root, 'real.json')
  writeFileSync(real, 'old', { mode: 0o600 })
  const link = join(root, 'settings.json')
  symlinkSync(real, link)
  await writeIfUnchanged(link, 'old', 'new')
  expect(lstatSync(link).isSymbolicLink()).toBe(true)
  expect(readlinkSync(link)).toBe(real)
  expect(readFileSync(real, 'utf8')).toBe('new')
  expect(modeOf(real)).toBe(0o600)
  expect(leftovers(root)).toEqual([])
})

it('writeIfUnchanged: creates the missing target of a dangling symlink', async () => {
  const real = join(root, 'missing', 'settings.json')
  const link = join(root, 'settings.json')
  symlinkSync(real, link)
  await writeIfUnchanged(link, null, 'text')
  expect(lstatSync(link).isSymbolicLink()).toBe(true)
  expect(existsSync(real)).toBe(true)
  expect(readFileSync(real, 'utf8')).toBe('text')
})
