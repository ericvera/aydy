import { expect, it } from 'vitest'

import {
  aydyDataDir,
  binDir,
  claudeConfigDir,
  claudeSessionsDir,
  claudeSettingsPath,
  HOOK_SCRIPT_MARKER,
  hookScriptPath,
  jumpDir,
  noticesDir,
  seenDir,
  statusDir,
  windowsDir,
  workspacesDir,
} from './aydyPaths'

const home = '/Users/someone'
const data = '/Users/someone/Library/Application Support/app.aydy'

it('places every AYDY directory under the data directory', () => {
  const options = { home, env: {} }

  expect(aydyDataDir(options)).toBe(data)
  expect(statusDir(options)).toBe(`${data}/status`)
  expect(seenDir(options)).toBe(`${data}/seen`)
  expect(jumpDir(options)).toBe(`${data}/jump`)
  expect(windowsDir(options)).toBe(`${data}/windows`)
  expect(workspacesDir(options)).toBe(`${data}/workspaces`)
  expect(noticesDir(options)).toBe(`${data}/notices`)
  expect(binDir(options)).toBe(`${data}/bin`)
  expect(hookScriptPath(options)).toBe(`${data}/bin/aydy-hook.sh`)
})

it('names the installed hook script with a path the marker matches', () => {
  expect(hookScriptPath({ home }).endsWith(HOOK_SCRIPT_MARKER)).toBe(true)
})

it('defaults the Claude config directory to ~/.claude', () => {
  const options = { home, env: {} }

  expect(claudeConfigDir(options)).toBe('/Users/someone/.claude')
  expect(claudeSettingsPath(options)).toBe(
    '/Users/someone/.claude/settings.json',
  )
  expect(claudeSessionsDir(options)).toBe('/Users/someone/.claude/sessions')
})

it('honours CLAUDE_CONFIG_DIR', () => {
  const options = { home, env: { CLAUDE_CONFIG_DIR: '/opt/claude' } }

  expect(claudeConfigDir(options)).toBe('/opt/claude')
  expect(claudeSettingsPath(options)).toBe('/opt/claude/settings.json')
  expect(claudeSessionsDir(options)).toBe('/opt/claude/sessions')
})

it('ignores an empty CLAUDE_CONFIG_DIR', () => {
  expect(claudeConfigDir({ home, env: { CLAUDE_CONFIG_DIR: '' } })).toBe(
    '/Users/someone/.claude',
  )
})
