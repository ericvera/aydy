export type SessionEvent =
  'SessionStart' | 'UserPromptSubmit' | 'Notification' | 'Stop'

export interface SessionStatus {
  sessionId: string
  event: SessionEvent
  cwd: string | null
  transcriptPath: string | null
  eventAt: number
  subagentsRunning: boolean
  informational: boolean
}

export interface WindowFolders {
  pid: number
  folders: string[]
}

export const SESSION_ID_PATTERN = /^[A-Za-z0-9_-]{1,128}$/

const EVENTS: ReadonlySet<string> = new Set<SessionEvent>([
  'SessionStart',
  'UserPromptSubmit',
  'Notification',
  'Stop',
])

const INFORMATIONAL_NOTIFICATION_TYPES: ReadonlySet<string> = new Set([
  'agent_completed',
  'auth_success',
])

const isEvent = (value: unknown): value is SessionEvent =>
  typeof value === 'string' && EVENTS.has(value)

const nonEmptyString = (value: unknown): string | null =>
  typeof value === 'string' && value !== '' ? value : null

const subagentsRunning = (value: unknown): boolean =>
  Array.isArray(value) &&
  value.some(
    (task: unknown) =>
      typeof task === 'object' &&
      task !== null &&
      (task as Record<string, unknown>)['type'] === 'subagent' &&
      (task as Record<string, unknown>)['status'] === 'running',
  )

const isInformational = (value: unknown): boolean =>
  typeof value === 'string' &&
  INFORMATIONAL_NOTIFICATION_TYPES.has(value.trim())

export const parseStatus = (
  text: string,
  mtimeMs: number,
): SessionStatus | null => {
  let payload: unknown
  try {
    payload = JSON.parse(text)
  } catch {
    return null
  }
  if (typeof payload !== 'object' || payload === null) {
    return null
  }
  const record = payload as Record<string, unknown>
  const sessionId = record['session_id']
  const event = record['hook_event_name']
  if (typeof sessionId !== 'string' || !SESSION_ID_PATTERN.test(sessionId)) {
    return null
  }
  if (!isEvent(event)) {
    return null
  }
  return {
    sessionId,
    event,
    cwd: nonEmptyString(record['cwd']),
    transcriptPath: nonEmptyString(record['transcript_path']),
    eventAt: mtimeMs,
    subagentsRunning:
      event === 'Stop' && subagentsRunning(record['background_tasks']),
    informational:
      event === 'Notification' && isInformational(record['notification_type']),
  }
}

export const isUnread = (
  status: SessionStatus,
  seenAt: number | undefined,
): boolean =>
  ((status.event === 'Stop' && !status.subagentsRunning) ||
    (status.event === 'Notification' && !status.informational)) &&
  status.eventAt > (seenAt ?? 0)

export const sessionDir = (
  status: SessionStatus,
  launchCwds: ReadonlyMap<string, string | null> | null,
): string | null => launchCwds?.get(status.sessionId) ?? status.cwd

const trimSeparators = (path: string): string => {
  const trimmed = path.replace(/\/+$/, '')
  return trimmed === '' ? '/' : trimmed
}

const contains = (folder: string, cwd: string): boolean =>
  folder === '/' || cwd === folder || cwd.startsWith(`${folder}/`)

export const containingFolderLength = (
  cwd: string | null,
  folders: string[],
): number => {
  if (cwd === null) {
    return -1
  }
  const target = trimSeparators(cwd)
  return folders
    .map(trimSeparators)
    .filter((folder) => contains(folder, target))
    .reduce((longest, folder) => Math.max(longest, folder.length), -1)
}

export const ownerOf = (
  cwd: string | null,
  windows: WindowFolders[],
): number | null => {
  let owner: number | null = null
  let best = -1
  for (const { pid, folders } of windows) {
    const length = containingFolderLength(cwd, folders)
    if (length < 0) {
      continue
    }
    if (length > best || (length === best && owner !== null && pid < owner)) {
      owner = pid
      best = length
    }
  }
  return owner
}

export const unreadCount = (
  statuses: readonly SessionStatus[],
  seen: ReadonlyMap<string, number>,
): number =>
  statuses.filter((status) => isUnread(status, seen.get(status.sessionId)))
    .length

export const isWorking = (status: SessionStatus): boolean =>
  status.event === 'UserPromptSubmit' ||
  (status.event === 'Stop' && status.subagentsRunning)

export interface TitleSignal {
  stale: boolean
  text: string
}

export interface WindowGlance {
  unread: number
  working: boolean
  signal: TitleSignal | null
}

export const windowGlance = (
  statuses: readonly SessionStatus[],
  seen: ReadonlyMap<string, number>,
  signal: TitleSignal | null = null,
): WindowGlance => ({
  unread: unreadCount(statuses, seen),
  working: statuses.some(isWorking),
  signal,
})

export const tabTitle = (glance: WindowGlance, label: string): string => {
  const { unread } = glance
  if (unread === 1) {
    return `● ${label}`
  }
  if (unread > 1) {
    return `● ${unread > 9 ? '9+' : String(unread)} ${label}`
  }
  const { signal } = glance
  if (signal !== null) {
    return `${signal.stale ? '◌' : '◐'} ${signal.text} ${label}`
  }
  return glance.working ? `◐ ${label}` : `○ ${label}`
}
