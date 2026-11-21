import type { WorkspaceDirs } from './workspaceFile'
import {
  legacyWorkspacePath,
  registerWorkspace,
  siblingWorkspacePath,
  workspaceState,
  writeWorkspace,
} from './workspaceFile'

/** What is open in a window that reopening it would end. */
export interface WindowContents {
  terminals: number
  claudeTabs: number
  claudeSessions: number
  dirtyDocuments: number
}

/** Why reopening this window now would lose something; none when safe. */
export const unsafeReasons = (contents: WindowContents): string[] => {
  const reasons: string[] = []
  if (contents.claudeTabs > 0 || contents.claudeSessions > 0) {
    reasons.push('Claude Code sessions')
  }
  if (contents.terminals > 0) {
    reasons.push(contents.terminals === 1 ? 'a terminal' : 'terminals')
  }
  if (contents.dirtyDocuments > 0) {
    reasons.push('unsaved files')
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
