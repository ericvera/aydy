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
): SessionStatus => ({ sessionId, event, cwd, eventAt })

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
      eventAt: 42,
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
  [{ unread: 0, working: false }, '○ aydy'],
  [{ unread: 0, working: true }, '◐ aydy'],
  [{ unread: 1, working: false }, '● aydy'],
  [{ unread: 2, working: false }, '● 2 aydy'],
  [{ unread: 9, working: false }, '● 9 aydy'],
  [{ unread: 10, working: false }, '● 9+ aydy'],
  [{ unread: 12, working: false }, '● 9+ aydy'],
  [{ unread: 1, working: true }, '● aydy'],
  [{ unread: 3, working: true }, '● 3 aydy'],
])('tabTitle(%j)', (glance, title) => {
  expect(tabTitle(glance, 'aydy')).toBe(title)
})

it('windowGlance is working only while a prompt runs', () => {
  const none = new Map<string, number>()
  expect(windowGlance([status('UserPromptSubmit', 100, 'a')], none)).toEqual({
    unread: 0,
    working: true,
  })
  expect(windowGlance([status('Notification', 100, 'a')], none)).toEqual({
    unread: 1,
    working: false,
  })
  expect(
    windowGlance([status('Stop', 100, 'a')], new Map([['a', 200]])),
  ).toEqual({ unread: 0, working: false })
  expect(
    windowGlance(
      [status('Stop', 100, 'a'), status('UserPromptSubmit', 100, 'b')],
      none,
    ),
  ).toEqual({ unread: 1, working: true })
})
