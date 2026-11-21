import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import {
  claimFailureNotice,
  failureMessage,
  releaseFailureNotice,
} from './hookNotice'

let root: string
let dir: string

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'aydy-notice-'))
  dir = join(root, 'notices')
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

it('claimFailureNotice: only the first claim wins', async () => {
  expect(await claimFailureNotice(dir)).toBe(true)
  expect(await claimFailureNotice(dir)).toBe(false)
})

it('releaseFailureNotice: lets the next claim win, and tolerates a missing marker', async () => {
  await releaseFailureNotice(dir)
  expect(await claimFailureNotice(dir)).toBe(true)
  await releaseFailureNotice(dir)
  expect(await claimFailureNotice(dir)).toBe(true)
})

it.each([
  ['malformed', 'The file is not valid JSON.'],
  ['unexpected', 'Its hooks section is not in the expected shape.'],
  ['unreadable', 'The file could not be read.'],
  ['unwritable', 'The file could not be written.'],
  ['changedOnDisk', 'It kept changing while AYDY was writing.'],
] as const)('failureMessage: %s', (reason, detail) => {
  expect(
    failureMessage(
      reason,
      '/Users/someone/.claude/settings.json',
      '/Users/someone',
    ),
  ).toBe(
    `AYDY could not add its hooks to ~/.claude/settings.json, so sessions are not being tracked. ${detail} Nothing in it was changed.`,
  )
})

it('failureMessage: leaves a path outside the home directory as is', () => {
  expect(
    failureMessage('malformed', '/etc/claude/settings.json', '/Users/someone'),
  ).toContain('add its hooks to /etc/claude/settings.json,')
})
