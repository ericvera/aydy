import { expect, it } from 'vitest'

import {
  containingFolderLength,
  isUnread,
  ownerOf,
  parseStatus,
  sessionDir,
  tabTitle,
  unreadCount,
  windowGlance,
} from './sessionState'
import type { SessionEvent, SessionStatus } from './sessionState'

const status = (
  event: SessionEvent,
  eventAt = 100,
  sessionId = 's1',
  cwd: string | null = '/a',
): SessionStatus => ({
  sessionId,
  event,
  cwd,
  transcriptPath: null,
  eventAt,
  subagentsRunning: false,
  informational: false,
})

it.each(['SessionStart', 'UserPromptSubmit', 'Notification', 'Stop'] as const)(
  'parseStatus: parses %s',
  (event) => {
    const text = JSON.stringify({
      session_id: 'abc-1_2',
      hook_event_name: event,
      cwd: '/a/b',
      notification_type: 'idle_prompt',
    })
    expect(parseStatus(text, 42)).toEqual({
      sessionId: 'abc-1_2',
      event,
      cwd: '/a/b',
      transcriptPath: null,
      eventAt: 42,
      subagentsRunning: false,
      informational: false,
    })
  },
)

it('parseStatus: gives a null cwd when absent or blank', () => {
  expect(
    parseStatus(JSON.stringify({ session_id: 's', hook_event_name: 'Stop' }), 1)
      ?.cwd,
  ).toBeNull()
  expect(
    parseStatus(
      JSON.stringify({ session_id: 's', hook_event_name: 'Stop', cwd: '' }),
      1,
    )?.cwd,
  ).toBeNull()
})

it('parseStatus: reads the transcript path, null when absent or blank', () => {
  const read = (transcript: unknown): string | null | undefined =>
    parseStatus(
      JSON.stringify({
        session_id: 's',
        hook_event_name: 'Stop',
        transcript_path: transcript,
      }),
      1,
    )?.transcriptPath
  expect(read('/p/s.jsonl')).toBe('/p/s.jsonl')
  expect(read('')).toBeNull()
  expect(read(undefined)).toBeNull()
})

it.each([
  [
    'unknown event',
    JSON.stringify({ session_id: 's', hook_event_name: 'PreToolUse' }),
  ],
  ['bad JSON', '{not json'],
  ['non-object JSON', '"Stop"'],
  ['missing id', JSON.stringify({ hook_event_name: 'Stop' })],
  ['blank id', JSON.stringify({ session_id: '', hook_event_name: 'Stop' })],
  ['path id', JSON.stringify({ session_id: '../x', hook_event_name: 'Stop' })],
  [
    'long id',
    JSON.stringify({ session_id: 'a'.repeat(129), hook_event_name: 'Stop' }),
  ],
])('parseStatus: rejects %s', (_label, text) => {
  expect(parseStatus(text, 1)).toBeNull()
})

it.each(['Stop', 'Notification'] as const)(
  'isUnread: %s is unread only when newer than seen',
  (event) => {
    expect(isUnread(status(event, 100), 100)).toBe(false)
    expect(isUnread(status(event, 100), 150)).toBe(false)
    expect(isUnread(status(event, 100), 50)).toBe(true)
    expect(isUnread(status(event, 100), undefined)).toBe(true)
  },
)

it.each(['UserPromptSubmit', 'SessionStart'] as const)(
  'isUnread: %s is never unread',
  (event) => {
    expect(isUnread(status(event, 100), undefined)).toBe(false)
    expect(isUnread(status(event, 100), 50)).toBe(false)
  },
)

it('containingFolderLength: compares on path segments', () => {
  expect(containingFolderLength('/a/bc', ['/a/b'])).toBe(-1)
  expect(containingFolderLength('/a/b/c', ['/a/b'])).toBe(4)
})

it('containingFolderLength: treats an equal path as containing', () => {
  expect(containingFolderLength('/a/b', ['/a/b'])).toBe(4)
})

it('containingFolderLength: ignores trailing separators', () => {
  expect(containingFolderLength('/a/b/', ['/a/b/'])).toBe(4)
  expect(containingFolderLength('/a/b/c', ['/a/b/'])).toBe(4)
})

it('containingFolderLength: returns the longest containing folder', () => {
  expect(containingFolderLength('/a/b/c', ['/a', '/a/b', '/x'])).toBe(4)
})

it('containingFolderLength: returns -1 for a null cwd', () => {
  expect(containingFolderLength(null, ['/a'])).toBe(-1)
})

const repo = { pid: 10, folders: ['/a'] }
const worktree = { pid: 20, folders: ['/a/.claude/worktrees/x'] }

it('ownerOf: gives a worktree session to the worktree window', () => {
  expect(ownerOf('/a/.claude/worktrees/x/src', [repo, worktree])).toBe(20)
  expect(ownerOf('/a/.claude/worktrees/x', [repo, worktree])).toBe(20)
})

it('ownerOf: gives a repo session to the repo window', () => {
  expect(ownerOf('/a', [worktree, repo])).toBe(10)
})

it('ownerOf: breaks ties toward the lower pid', () => {
  expect(
    ownerOf('/a', [
      { pid: 30, folders: ['/a'] },
      { pid: 5, folders: ['/a'] },
    ]),
  ).toBe(5)
  expect(
    ownerOf('/a', [
      { pid: 5, folders: ['/a'] },
      { pid: 30, folders: ['/a'] },
    ]),
  ).toBe(5)
})

it('ownerOf: returns null when no window contains the directory', () => {
  expect(ownerOf('/b', [repo, worktree])).toBeNull()
  expect(ownerOf(null, [repo])).toBeNull()
})

const moved = status('Stop', 100, 's1', '/a/.claude/worktrees/x')
const windows = [
  { pid: 10, folders: ['/a'] },
  { pid: 20, folders: ['/a/.claude/worktrees/x'] },
]

it('sessionDir: prefers the launch directory', () => {
  const dir = sessionDir(moved, new Map([['s1', '/a']]))
  expect(dir).toBe('/a')
  expect(ownerOf(dir, windows)).toBe(10)
})

it('sessionDir: falls back to the payload cwd', () => {
  expect(sessionDir(moved, new Map([['other', '/a']]))).toBe(
    '/a/.claude/worktrees/x',
  )
  expect(sessionDir(moved, new Map([['s1', null]]))).toBe(
    '/a/.claude/worktrees/x',
  )
  expect(sessionDir(moved, null)).toBe('/a/.claude/worktrees/x')
})

it('unreadCount counts only unread statuses', () => {
  const statuses = [
    status('Stop', 100, 'a'),
    status('Notification', 100, 'b'),
    status('Stop', 100, 'c'),
    status('UserPromptSubmit', 100, 'd'),
    status('SessionStart', 100, 'e'),
  ]
  expect(unreadCount(statuses, new Map([['c', 200]]))).toBe(2)
})

it.each([
  [{ unread: 0, working: false, signal: null }, '○ aydy'],
  [{ unread: 0, working: true, signal: null }, '◐ aydy'],
  [{ unread: 1, working: false, signal: null }, '● aydy'],
  [{ unread: 2, working: false, signal: null }, '● 2 aydy'],
  [{ unread: 9, working: false, signal: null }, '● 9 aydy'],
  [{ unread: 10, working: false, signal: null }, '● 9+ aydy'],
  [{ unread: 12, working: false, signal: null }, '● 9+ aydy'],
  [{ unread: 1, working: true, signal: null }, '● aydy'],
  [{ unread: 3, working: true, signal: null }, '● 3 aydy'],
])('tabTitle(%j)', (glance, title) => {
  expect(tabTitle(glance, 'aydy')).toBe(title)
})

it.each([
  [{ stale: false, text: '4/7 · ~6m' }, 0, '◐ 4/7 · ~6m label'],
  [{ stale: true, text: 'quiet 9m' }, 0, '◌ quiet 9m label'],
  [{ stale: true, text: 'quiet 9m' }, 1, '● label'],
  [{ stale: false, text: '4/7 · ~6m' }, 2, '● 2 label'],
])('tabTitle: signal %j with %i unread', (signal, unread, title) => {
  expect(tabTitle({ unread, working: true, signal }, 'label')).toBe(title)
})

it('windowGlance is working only while a prompt runs', () => {
  const none = new Map<string, number>()
  expect(windowGlance([status('UserPromptSubmit', 100, 'a')], none)).toEqual({
    unread: 0,
    working: true,
    signal: null,
  })
  expect(windowGlance([status('Notification', 100, 'a')], none)).toEqual({
    unread: 1,
    working: false,
    signal: null,
  })
  expect(
    windowGlance([status('Stop', 100, 'a')], new Map([['a', 200]])),
  ).toEqual({ unread: 0, working: false, signal: null })
  expect(
    windowGlance(
      [status('Stop', 100, 'a'), status('UserPromptSubmit', 100, 'b')],
      none,
    ),
  ).toEqual({ unread: 1, working: true, signal: null })
})

const parsed = (fields: Record<string, unknown>): SessionStatus => {
  const result = parseStatus(
    JSON.stringify({ session_id: 's1', cwd: '/a', ...fields }),
    100,
  )
  if (result === null) {
    throw new Error('expected a status')
  }
  return result
}

const none = new Map<string, number>()

it('parseStatus: a Stop with a running subagent is working, not unread', () => {
  const result = parsed({
    hook_event_name: 'Stop',
    background_tasks: [{ type: 'subagent', status: 'running' }],
  })
  expect(result.subagentsRunning).toBe(true)
  expect(isUnread(result, undefined)).toBe(false)
  const glance = windowGlance([result], none)
  expect(glance).toEqual({ unread: 0, working: true, signal: null })
  expect(tabTitle(glance, 'aydy')).toBe('◐ aydy')
})

it.each([
  ['only a running shell', [{ type: 'shell', status: 'running' }]],
  ['a finished subagent', [{ type: 'subagent', status: 'completed' }]],
  ['no tasks', []],
  ['a malformed value', 'running'],
  ['malformed entries', [null, 'subagent', 7]],
  ['no field', undefined],
])('parseStatus: a Stop with %s is unread, not working', (_label, tasks) => {
  const result = parsed({ hook_event_name: 'Stop', background_tasks: tasks })
  expect(result.subagentsRunning).toBe(false)
  expect(windowGlance([result], none)).toEqual({
    unread: 1,
    working: false,
    signal: null,
  })
})

it.each(['agent_completed', 'auth_success', ' agent_completed\n'])(
  'parseStatus: a %s notification is informational, neither unread nor working',
  (type) => {
    const result = parsed({
      hook_event_name: 'Notification',
      notification_type: type,
    })
    expect(result.informational).toBe(true)
    expect(windowGlance([result], none)).toEqual({
      unread: 0,
      working: false,
      signal: null,
    })
  },
)

it.each([
  ['permission_prompt', 'permission_prompt'],
  ['no type', undefined],
  ['a non-string type', ['agent_completed']],
])('parseStatus: a notification with %s is still unread', (_label, type) => {
  const result = parsed({
    hook_event_name: 'Notification',
    notification_type: type,
  })
  expect(result.informational).toBe(false)
  expect(isUnread(result, undefined)).toBe(true)
})
