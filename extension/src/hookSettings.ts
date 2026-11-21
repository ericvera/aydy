import { HOOK_SCRIPT_MARKER } from './aydyPaths'

export const HOOK_EVENTS = [
  'SessionStart',
  'UserPromptSubmit',
  'Notification',
  'Stop',
  'SessionEnd',
] as const

export type HookEvent = (typeof HOOK_EVENTS)[number]

const DEFAULT_TIMEOUT_SECONDS = 5

/** SessionEnd hooks share a 1.5 s machine-wide budget. */
const SESSION_END_TIMEOUT_SECONDS = 1

export class UnexpectedShape extends Error {
  constructor(where: string) {
    super(`Unexpected shape in Claude settings at ${where}`)
    this.name = 'UnexpectedShape'
  }
}

type Json = Record<string, unknown>

const isRecord = (value: unknown): value is Json =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

export const hookCommand = (): string =>
  `f="$HOME/Library/Application Support/${HOOK_SCRIPT_MARKER}"; [ -f "$f" ] && /bin/sh "$f"; exit 0`

const timeoutFor = (event: HookEvent): number =>
  event === 'SessionEnd' ? SESSION_END_TIMEOUT_SECONDS : DEFAULT_TIMEOUT_SECONDS

const aydyGroup = (event: HookEvent): Json => ({
  hooks: [
    { type: 'command', command: hookCommand(), timeout: timeoutFor(event) },
  ],
})

export const isAydyHook = (hook: unknown): boolean =>
  isRecord(hook) &&
  typeof hook['command'] === 'string' &&
  hook['command'].includes(HOOK_SCRIPT_MARKER)

const isExactHook = (hook: unknown): boolean =>
  isRecord(hook) && hook['command'] === hookCommand()

const hooksOf = (doc: Json): Json | undefined => {
  const hooks = doc['hooks']
  if (hooks === undefined) {
    return undefined
  }
  if (!isRecord(hooks)) {
    throw new UnexpectedShape('hooks')
  }
  return hooks
}

const groupsOf = (hooks: Json, event: string): unknown[] | undefined => {
  const groups = hooks[event]
  if (groups === undefined) {
    return undefined
  }
  if (!Array.isArray(groups)) {
    throw new UnexpectedShape(`hooks.${event}`)
  }
  return groups
}

const entriesOf = (group: unknown, where: string): unknown[] => {
  if (!isRecord(group) || !Array.isArray(group['hooks'])) {
    throw new UnexpectedShape(`${where}.hooks`)
  }
  return group['hooks']
}

const validate = (hooks: Json): void => {
  for (const event of Object.keys(hooks)) {
    groupsOf(hooks, event)?.forEach((group, index) =>
      entriesOf(group, `hooks.${event}[${index}]`),
    )
  }
}

const eventHasExactHook = (groups: unknown[], event: string): boolean =>
  groups.some((group, index) =>
    entriesOf(group, `hooks.${event}[${index}]`).some(isExactHook),
  )

export const hasAllHooks = (doc: unknown): boolean => {
  if (!isRecord(doc)) {
    throw new UnexpectedShape('root')
  }
  const hooks = hooksOf(doc)
  if (hooks === undefined) {
    return false
  }
  validate(hooks)
  return HOOK_EVENTS.every((event) => {
    const groups = groupsOf(hooks, event)
    return groups !== undefined && eventHasExactHook(groups, event)
  })
}

const pruneGroups = (
  groups: unknown[],
  event: string,
  remove: (hook: unknown) => boolean,
): unknown[] =>
  groups.flatMap((group, index) => {
    const entries = entriesOf(group, `hooks.${event}[${index}]`)
    if (!entries.some(remove)) {
      return [group]
    }
    const kept = entries.filter((hook) => !remove(hook))
    return kept.length === 0 ? [] : [{ ...(group as Json), hooks: kept }]
  })

const isStaleAydyHook = (hook: unknown): boolean =>
  isAydyHook(hook) && !isExactHook(hook)

export const withAydyHooks = (doc: Json): Json => {
  const hooks = hooksOf(doc) ?? {}
  validate(hooks)
  const next: Json = {}
  for (const [event, value] of Object.entries(hooks)) {
    const groups = pruneGroups(value as unknown[], event, isStaleAydyHook)
    if (groups.length > 0 || (value as unknown[]).length === 0) {
      next[event] = groups
    }
  }
  for (const event of HOOK_EVENTS) {
    const groups = (next[event] as unknown[] | undefined) ?? []
    if (!eventHasExactHook(groups, event)) {
      next[event] = [...groups, aydyGroup(event)]
    }
  }
  return { ...doc, hooks: next }
}

export const withoutAydyHooks = (doc: Json): Json => {
  const hooks = hooksOf(doc)
  if (hooks === undefined) {
    return { ...doc }
  }
  validate(hooks)
  const next: Json = {}
  for (const [event, value] of Object.entries(hooks)) {
    const groups = pruneGroups(value as unknown[], event, isAydyHook)
    if (groups.length > 0 || (value as unknown[]).length === 0) {
      next[event] = groups
    }
  }
  if (Object.keys(next).length > 0 || Object.keys(hooks).length === 0) {
    return { ...doc, hooks: next }
  }
  return Object.fromEntries(
    Object.entries(doc).filter(([key]) => key !== 'hooks'),
  )
}
