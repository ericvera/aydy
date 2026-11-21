import { expect, it } from 'vitest'

import {
  hasAllHooks,
  HOOK_EVENTS,
  hookCommand,
  isAydyHook,
  UnexpectedShape,
  withAydyHooks,
  withoutAydyHooks,
} from './hookSettings'

const FUNK = {
  hooks: [
    { type: 'command', command: 'afplay /System/Library/Sounds/Funk.aiff' },
  ],
}

const HERO = {
  matcher: '*',
  hooks: [
    { type: 'command', command: 'afplay /System/Library/Sounds/Hero.aiff' },
  ],
}

const LEGACY_HTTP = {
  hooks: [
    {
      type: 'http',
      url: 'http://127.0.0.1:47823/hook',
      headers: { 'X-Aydy-Hook': '1' },
    },
  ],
}

const realSettings = (): Record<string, unknown> => ({
  permissions: {
    allow: ['Bash(git status:*)', 'Read(//Users/eric/**)'],
    deny: ['Bash(rm -rf:*)'],
  },
  hooks: {
    Stop: [FUNK, LEGACY_HTTP],
    Notification: [HERO],
  },
  enabledPlugins: { 'mise@ericvera': true },
  extraKnownMarketplaces: {
    ericvera: {
      source: { source: 'github', repo: 'ericvera/claude-plugins' },
    },
  },
  effortLevel: 'high',
  tui: { theme: 'dark' },
  model: 'opus',
})

const aydyGroup = (timeout: number): Record<string, unknown> => ({
  hooks: [{ type: 'command', command: hookCommand(), timeout }],
})

const groups = (doc: Record<string, unknown>, event: string): unknown[] =>
  (doc['hooks'] as Record<string, unknown[]>)[event] ?? []

const expectUserHooksIntact = (doc: Record<string, unknown>): void => {
  expect(groups(doc, 'Stop').slice(0, 2)).toEqual([FUNK, LEGACY_HTTP])
  expect(groups(doc, 'Notification')[0]).toEqual(HERO)
}

it('hookCommand: runs the installed script when present and always exits 0', () => {
  expect(hookCommand()).toBe(
    'f="$HOME/Library/Application Support/app.aydy/bin/aydy-hook.sh"; [ -f "$f" ] && /bin/sh "$f"; exit 0',
  )
  expect(isAydyHook({ type: 'command', command: hookCommand() })).toBe(true)
  expect(isAydyHook(FUNK.hooks[0])).toBe(false)
  expect(isAydyHook(LEGACY_HTTP.hooks[0])).toBe(false)
})

it('withAydyHooks: adds one group per event to an empty doc', () => {
  const next = withAydyHooks({})
  expect(next).toEqual({
    hooks: {
      SessionStart: [aydyGroup(5)],
      UserPromptSubmit: [aydyGroup(5)],
      Notification: [aydyGroup(5)],
      Stop: [aydyGroup(5)],
      SessionEnd: [aydyGroup(1)],
    },
  })
  expect(hasAllHooks(next)).toBe(true)
})

it('withAydyHooks: adds alongside the user hooks and leaves them and the key order untouched', () => {
  const doc = realSettings()
  const next = withAydyHooks(doc)
  expect(Object.keys(next)).toEqual(Object.keys(doc))
  expect(next['permissions']).toEqual(doc['permissions'])
  expectUserHooksIntact(next)
  expect(groups(next, 'Stop')).toEqual([FUNK, LEGACY_HTTP, aydyGroup(5)])
  expect(groups(next, 'Notification')).toEqual([HERO, aydyGroup(5)])
  expect(doc).toEqual(realSettings())
  expect(hasAllHooks(doc)).toBe(false)
  expect(hasAllHooks(next)).toBe(true)
})

it('withAydyHooks: is a no-op when every hook is present', () => {
  const once = withAydyHooks(realSettings())
  expect(hasAllHooks(once)).toBe(true)
  const twice = withAydyHooks(once)
  expect(twice).toEqual(once)
  expectUserHooksIntact(twice)
})

it('withAydyHooks: replaces a stale AYDY command', () => {
  const stale = {
    type: 'command',
    command: '/bin/sh "/old/app.aydy/bin/aydy-hook.sh"',
    timeout: 5,
  }
  const doc = realSettings()
  ;(doc['hooks'] as Record<string, unknown[]>)['SessionStart'] = [
    { hooks: [stale] },
  ]
  expect(hasAllHooks(doc)).toBe(false)
  const next = withAydyHooks(doc)
  expect(groups(next, 'SessionStart')).toEqual([aydyGroup(5)])
  expectUserHooksIntact(next)
  for (const event of HOOK_EVENTS) {
    expect(
      groups(next, event).filter((group) =>
        JSON.stringify(group).includes('aydy-hook.sh'),
      ),
    ).toHaveLength(1)
  }
})

it('withoutAydyHooks: removes AYDY hooks, prunes empty groups and events, and keeps the user hooks', () => {
  const doc = withAydyHooks(realSettings())
  const next = withoutAydyHooks(doc)
  expect(next).toEqual(realSettings())
  expect(Object.keys(next)).toEqual(Object.keys(realSettings()))
  expectUserHooksIntact(next)
})

it('withoutAydyHooks: keeps sibling hooks in a shared group', () => {
  const shared = {
    hooks: [
      FUNK.hooks[0],
      { type: 'command', command: hookCommand(), timeout: 5 },
    ],
  }
  const next = withoutAydyHooks({ hooks: { Stop: [shared, LEGACY_HTTP] } })
  expect(next).toEqual({ hooks: { Stop: [FUNK, LEGACY_HTTP] } })
})

it('withoutAydyHooks: drops hooks itself when it becomes empty', () => {
  expect(withoutAydyHooks(withAydyHooks({ model: 'opus' }))).toEqual({
    model: 'opus',
  })
})

it.each([
  ['hooks', { hooks: [] }],
  ['an event value', { hooks: { Stop: {} } }],
  ['a group', { hooks: { Stop: ['x'] } }],
  ['a group hooks list', { hooks: { Stop: [{ hooks: {} }] } }],
])('wrong-shape input: throws UnexpectedShape for %s', (_label, doc) => {
  expect(() => withAydyHooks(doc)).toThrow(UnexpectedShape)
  expect(() => withoutAydyHooks(doc)).toThrow(UnexpectedShape)
  expect(() => hasAllHooks(doc)).toThrow(UnexpectedShape)
})
