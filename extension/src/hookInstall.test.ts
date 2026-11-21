import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import type { PathOptions } from './aydyPaths'
import { claudeSettingsPath, hookScriptPath } from './aydyPaths'
import { ensureHooks, installHookScript, removeHooks } from './hookInstall'
import { HOOK_EVENTS, hookCommand } from './hookSettings'

let home: string
let extensionPath: string
let paths: PathOptions

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), 'aydy-install-'))
  extensionPath = join(home, 'extension')
  mkdirSync(join(extensionPath, 'hook'), { recursive: true })
  writeFileSync(join(extensionPath, 'hook', 'aydy-hook.sh'), 'echo one\n')
  paths = { home, env: {} }
})

afterEach(() => {
  rmSync(home, { force: true, recursive: true })
})

const settingsFile = (): string => claudeSettingsPath(paths)

const readSettings = (): Record<string, Record<string, unknown[]>> =>
  JSON.parse(readFileSync(settingsFile(), 'utf8')) as Record<
    string,
    Record<string, unknown[]>
  >

const aydyEntries = (event: string): number =>
  (readSettings()['hooks']?.[event] ?? []).filter((group) =>
    JSON.stringify(group).includes(JSON.stringify(hookCommand())),
  ).length

it('installHookScript: copies the script with mode 755 and rewrites it only when the source differs', async () => {
  expect(await installHookScript(extensionPath, paths)).toBe(true)
  const target = hookScriptPath(paths)
  expect(readFileSync(target, 'utf8')).toBe('echo one\n')
  expect(statSync(target).mode & 0o777).toBe(0o755)

  expect(await installHookScript(extensionPath, paths)).toBe(false)

  writeFileSync(join(extensionPath, 'hook', 'aydy-hook.sh'), 'echo two\n')
  expect(await installHookScript(extensionPath, paths)).toBe(true)
  expect(readFileSync(target, 'utf8')).toBe('echo two\n')
  expect(statSync(target).mode & 0o777).toBe(0o755)
})

it('ensureHooks: creates .claude/settings.json with the five entries when .claude is missing', async () => {
  expect(await ensureHooks(paths)).toEqual({ ok: true, changed: true })
  expect(Object.keys(readSettings()['hooks'] ?? {})).toEqual([...HOOK_EVENTS])
  for (const event of HOOK_EVENTS) {
    expect(aydyEntries(event)).toBe(1)
  }
  expect(readFileSync(settingsFile(), 'utf8').endsWith('}\n')).toBe(true)
})

it('ensureHooks: writes once, then reports no change without touching the file', async () => {
  expect(await ensureHooks(paths)).toEqual({ ok: true, changed: true })
  const past = new Date('2020-01-01T00:00:00Z')
  utimesSync(settingsFile(), past, past)
  const before = statSync(settingsFile()).mtimeMs
  expect(await ensureHooks(paths)).toEqual({ ok: true, changed: false })
  expect(statSync(settingsFile()).mtimeMs).toBe(before)
})

it('ensureHooks: keeps the file indentation and trailing newline', async () => {
  mkdirSync(join(home, '.claude'))
  writeFileSync(settingsFile(), '{\n\t"model": "opus"\n}')
  await ensureHooks(paths)
  const text = readFileSync(settingsFile(), 'utf8')
  expect(text.startsWith('{\n\t"model": "opus",\n\t"hooks"')).toBe(true)
  expect(text.endsWith('}')).toBe(true)
})

it('ensureHooks: reports malformed JSON and leaves the file unchanged', async () => {
  mkdirSync(join(home, '.claude'))
  writeFileSync(settingsFile(), '{ "model": ')
  expect(await ensureHooks(paths)).toEqual({ ok: false, reason: 'malformed' })
  expect(readFileSync(settingsFile(), 'utf8')).toBe('{ "model": ')
})

it('ensureHooks: reports an unexpected shape and leaves the file unchanged', async () => {
  mkdirSync(join(home, '.claude'))
  writeFileSync(settingsFile(), '{"hooks":[]}')
  expect(await ensureHooks(paths)).toEqual({
    ok: false,
    reason: 'unexpected',
  })
  expect(readFileSync(settingsFile(), 'utf8')).toBe('{"hooks":[]}')
})

it('ensureHooks: converges to one entry per event under concurrent calls', async () => {
  const results = await Promise.all([ensureHooks(paths), ensureHooks(paths)])
  expect(results.every((result) => result.ok)).toBe(true)
  for (const event of HOOK_EVENTS) {
    expect(aydyEntries(event)).toBe(1)
  }
})

it('ensureHooks: keeps a 0600 settings file private through ensure and remove', async () => {
  mkdirSync(join(home, '.claude'))
  writeFileSync(settingsFile(), '{"model":"opus"}\n')
  chmodSync(settingsFile(), 0o600)

  expect(await ensureHooks(paths)).toEqual({ ok: true, changed: true })
  expect(statSync(settingsFile()).mode & 0o777).toBe(0o600)

  expect(await removeHooks(paths)).toEqual({ ok: true, changed: true })
  expect(statSync(settingsFile()).mode & 0o777).toBe(0o600)
  expect(readSettings()).toEqual({ model: 'opus' })
})

it('removeHooks: is a no-op when the file is missing', async () => {
  expect(await removeHooks(paths)).toEqual({ ok: true, changed: false })
})
