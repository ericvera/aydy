import { spawnSync } from 'node:child_process'
import {
  existsSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

const SCRIPT = join(__dirname, '..', 'hook', 'aydy-hook.sh')

const SESSION = 'c4f1d6a2-0f1e-4b1a-9d5e-3a2b1c0d9e8f'

let home: string

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), 'aydy-hook-'))
})

afterEach(() => {
  rmSync(home, { force: true, recursive: true })
})

const statusDir = (): string =>
  join(home, 'Library', 'Application Support', 'app.aydy', 'status')

const statusEntries = (): string[] =>
  existsSync(statusDir()) ? readdirSync(statusDir()) : []

const runHook = (input: string): void => {
  const result = spawnSync('/bin/sh', [SCRIPT], {
    env: { HOME: home, PATH: process.env['PATH'] },
    input,
  })
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toBe('')
  expect(result.stderr.toString()).toBe('')
  expect(statusEntries().filter((name) => name.startsWith('.hook.'))).toEqual(
    [],
  )
}

const payload = (fields: Record<string, string>): string =>
  JSON.stringify(fields)

const stop = payload({
  cwd: '/Users/someone/project',
  hook_event_name: 'Stop',
  session_id: SESSION,
  transcript_path: `/Users/someone/.claude/projects/p/${SESSION}.jsonl`,
})

it.each([
  ['ending in a newline', `${stop}\n`],
  ['ending in a brace', stop],
])('writes the payload byte for byte when it is %s', (_label, input) => {
  runHook(input)

  expect(readFileSync(join(statusDir(), `${SESSION}.json`), 'utf8')).toBe(input)
})

it('removes the status file on SessionEnd', () => {
  runHook(`${stop}\n`)
  runHook(
    `${payload({ hook_event_name: 'SessionEnd', session_id: SESSION })}\n`,
  )

  expect(statusEntries()).toEqual([])
})

it.each([
  ['no session id', `${payload({ hook_event_name: 'Stop' })}\n`],
  ['invalid JSON', 'not json at all'],
  [
    'an id with a slash',
    `${payload({ hook_event_name: 'Stop', session_id: 'a/b' })}\n`,
  ],
  [
    'an id with dots',
    `${payload({ hook_event_name: 'Stop', session_id: '..' })}\n`,
  ],
])('writes nothing for %s', (_label, input) => {
  runHook(input)

  expect(statusEntries()).toEqual([])
})

it('ignores a session id embedded in a string value', () => {
  const input = `${JSON.stringify({
    hook_event_name: 'Notification',
    session_id: SESSION,
    message: 'look: "session_id":"evil"',
  })}\n`

  runHook(input)

  expect(statusEntries()).toEqual([`${SESSION}.json`])
})

const notification = (type: string): string =>
  `${payload({
    hook_event_name: 'Notification',
    notification_type: type,
    session_id: SESSION,
  })}\n`

it.each(['agent_completed', 'auth_success'])(
  'keeps the previous payload for a %s notification',
  (type) => {
    runHook(`${stop}\n`)
    runHook(notification(type))

    expect(readFileSync(join(statusDir(), `${SESSION}.json`), 'utf8')).toBe(
      `${stop}\n`,
    )
  },
)

it('overwrites the previous payload for a permission prompt', () => {
  runHook(`${stop}\n`)
  runHook(notification('permission_prompt'))

  expect(readFileSync(join(statusDir(), `${SESSION}.json`), 'utf8')).toBe(
    notification('permission_prompt'),
  )
})
