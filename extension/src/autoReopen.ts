import type { SessionStatus, WindowFolders } from './sessionState'
import { isWorking } from './sessionState'
import type { WorkspaceDirs } from './workspaceFile'
import {
  legacyWorkspacePath,
  registerWorkspace,
  siblingWorkspacePath,
  workspaceState,
  writeWorkspace,
} from './workspaceFile'

/** What is running in a window that reopening it would lose. */
export interface WindowContents {
  activeSessions: number
  busyTerminals: number
  dirtyDocuments: number
  checkFailed: boolean
}

/** Whether a session is working or waiting on a real notification. */
export const isActive = (status: SessionStatus | undefined): boolean =>
  status !== undefined &&
  (isWorking(status) ||
    (status.event === 'Notification' && !status.informational))

/** How many of `sessionIds` are working or waiting; no status means idle. */
export const countActive = (
  sessionIds: readonly string[],
  statuses: readonly SessionStatus[],
): number => {
  const byId = new Map(statuses.map((status) => [status.sessionId, status]))
  return sessionIds.filter((id) => isActive(byId.get(id))).length
}

/** Why reopening this window now would lose something; none when safe. */
export const unsafeReasons = (contents: WindowContents): string[] => {
  const reasons: string[] = []
  if (contents.activeSessions > 0) {
    reasons.push(
      contents.activeSessions === 1
        ? 'a Claude Code session that is working or waiting on you'
        : 'Claude Code sessions that are working or waiting on you',
    )
  }
  if (contents.busyTerminals > 0) {
    reasons.push(
      contents.busyTerminals === 1
        ? 'a terminal running a process'
        : 'terminals running processes',
    )
  }
  if (contents.dirtyDocuments > 0) {
    reasons.push('unsaved files')
  }
  if (contents.checkFailed) {
    reasons.push('processes AYDY could not check')
  }
  return reasons
}

/** Joins reasons as prose: `a`, `a and b`, `a, b and c`. */
export const listReasons = (reasons: readonly string[]): string =>
  reasons.length <= 1
    ? (reasons[0] ?? '')
    : `${reasons.slice(0, -1).join(', ')} and ${reasons.at(-1) ?? ''}`

export interface ReopenEnvironment {
  openFolder(
    path: string,
    options: { forceReuseWindow: boolean },
  ): Thenable<unknown>
  /** Asks whether to overwrite a file at `path` that AYDY did not write. */
  confirmOverwrite(path: string): Thenable<boolean>
  dirs?: WorkspaceDirs
}

/**
 * Picks where `folder`'s workspace file goes: beside it, unless something AYDY
 * did not write is already there and the user declines to overwrite it, in
 * which case it goes to AYDY's own data directory.
 */
export const chooseWorkspacePath = async (
  folder: string,
  env: Pick<ReopenEnvironment, 'confirmOverwrite' | 'dirs'>,
): Promise<{ path: string; fresh: boolean }> => {
  const sibling = siblingWorkspacePath(folder)
  const state = await workspaceState(sibling, folder)
  if (state !== 'foreign') {
    return { path: sibling, fresh: false }
  }
  if (await env.confirmOverwrite(sibling)) {
    return { path: sibling, fresh: true }
  }
  return { path: legacyWorkspacePath(folder, env.dirs), fresh: false }
}

/** Writes `folder`'s workspace file and reopens this window on it. */
export const reopenAsWorkspace = async (
  folder: string,
  title: string,
  env: ReopenEnvironment,
): Promise<string> => {
  const { path, fresh } = await chooseWorkspacePath(folder, env)
  await writeWorkspace(path, folder, title, { fresh })
  await registerWorkspace(path, folder, env.dirs)
  await env.openFolder(path, { forceReuseWindow: true })
  return path
}

/** The workspace file of `folder` another window already has open, if any. */
export const workspaceOpenElsewhere = (
  folder: string,
  selfPid: number,
  windows: readonly WindowFolders[],
  dirs?: WorkspaceDirs,
): string | null => {
  const paths = [
    siblingWorkspacePath(folder),
    legacyWorkspacePath(folder, dirs),
  ]
  const other = windows.find(
    ({ pid, workspace }) =>
      pid !== selfPid &&
      typeof workspace === 'string' &&
      paths.includes(workspace),
  )
  return other?.workspace ?? null
}
