import { type SpawnSyncReturns, spawnSync } from 'node:child_process'
import {
  accessSync,
  constants,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

const SCRIPT = join(
  __dirname,
  '..',
  '..',
  'plugin',
  'skills',
  'progress',
  'scripts',
  'aydy-progress',
)

const SESSION = 'c4f1d6a2-0f1e-4b1a-9d5e-3a2b1c0d9e8f'

let home: string

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), 'aydy-progress-'))
})

afterEach(() => {
  rmSync(home, { force: true, recursive: true })
})

const appDir = (): string =>
  join(home, 'Library', 'Application Support', 'app.aydy')

const progressDir = (): string => join(appDir(), 'progress')

const progressEntries = (): string[] =>
  existsSync(progressDir()) ? readdirSync(progressDir()) : []

const installAydy = (): void => {
  mkdirSync(appDir(), { recursive: true })
}

const run = (
  args: string[],
  session: string | null = SESSION,
): SpawnSyncReturns<string> =>
  spawnSync('/bin/sh', [SCRIPT, ...args], {
    encoding: 'utf8',
    env: {
      HOME: home,
      PATH: process.env['PATH'],
      ...(session === null ? {} : { CLAUDE_CODE_SESSION_ID: session }),
    },
  })

const report = (): string =>
  readFileSync(join(progressDir(), `${SESSION}.json`), 'utf8')

it('writes the eta and step and says what it reported', () => {
  installAydy()

  const result = run(['--eta', '6m', '--step', '4/7'])

  expect(result.status).toBe(0)
  expect(result.stdout).toBe('Reported to AYDY: 4/7 · ~6m left\n')
  expect(result.stderr).toBe('')
  expect(report()).toBe('{"etaSeconds":360,"step":{"done":4,"total":7}}')
  expect(progressEntries()).toEqual([`${SESSION}.json`])
})

it.each([['1h 10m'], ['1h10m'], ['--eta=1h 10m']])(
  'reads %s as 4200 seconds',
  (eta) => {
    installAydy()

    const result = run(eta.startsWith('--') ? [eta] : ['--eta', eta])

    expect(result.status).toBe(0)
    expect(result.stdout).toBe('Reported to AYDY: ~1h 10m left\n')
    expect(report()).toBe('{"etaSeconds":4200}')
  },
)

it('omits the step when none is given and writes seconds as seconds', () => {
  installAydy()

  const result = run(['--eta', '45s'])

  expect(result.status).toBe(0)
  expect(result.stdout).toBe('Reported to AYDY: ~45s left\n')
  expect(report()).toBe('{"etaSeconds":45}')
})

it('replaces the previous report', () => {
  installAydy()

  run(['--eta', '6m', '--step', '4/7'])
  const result = run(['--eta', '2m', '--step=6/7'])

  expect(result.stdout).toBe('Reported to AYDY: 6/7 · ~2m left\n')
  expect(report()).toBe('{"etaSeconds":120,"step":{"done":6,"total":7}}')
  expect(progressEntries()).toEqual([`${SESSION}.json`])
})

it.each([
  ['an eta without a unit', ['--eta', '6']],
  ['an eta with an unknown unit', ['--eta', '6x']],
  ['a negative eta', ['--eta', '-1m']],
  ['an eta over a day', ['--eta', '25h']],
  ['units out of order', ['--eta', '10m 1h']],
  ['an empty eta', ['--eta=']],
  ['a step past its total', ['--eta', '6m', '--step', '5/4']],
  ['a step total of zero', ['--eta', '6m', '--step', '1/0']],
  ['a non-numeric step', ['--eta', '6m', '--step', 'a/b']],
  ['an unknown flag', ['--eta', '6m', '--doing', 'x']],
  ['a missing eta', ['--step', '4/7']],
  ['a repeated eta', ['--eta', '6m', '--eta', '2m']],
])('exits 2 and writes nothing for %s', (_label, args) => {
  installAydy()

  const result = run(args)

  expect(result.status).toBe(2)
  expect(result.stderr).not.toBe('')
  expect(progressEntries()).toEqual([])
})

it('prints usage for --help', () => {
  const result = run(['--help'])

  expect(result.status).toBe(0)
  expect(result.stdout).toContain('usage: aydy-progress --eta')
})

it.each([
  ['no session id', null],
  ['an invalid session id', '../x'],
  ['an over-long session id', 'a'.repeat(129)],
])('exits 1 and writes nothing for %s', (_label, session) => {
  installAydy()

  const result = run(['--eta', '6m'], session)

  expect(result.status).toBe(1)
  expect(result.stderr).toBe(
    'aydy-progress: not running inside a Claude Code session; nothing reported\n',
  )
  expect(progressEntries()).toEqual([])
})

it('creates nothing when AYDY is not installed', () => {
  const result = run(['--eta', '6m'])

  expect(result.status).toBe(0)
  expect(result.stdout).toBe('AYDY is not installed here; nothing reported\n')
  expect(existsSync(join(home, 'Library'))).toBe(false)
})

it('is executable', () => {
  expect(() => {
    accessSync(SCRIPT, constants.X_OK)
  }).not.toThrow()
})
