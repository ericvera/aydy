// Mirrors the repository rules in src-tauri/src/discovery/repo.rs, so a window
// title names a repository the way the menubar does.
import { readFile, stat } from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'

export interface RepoIdentity {
  root: string
  name: string
  worktree: string | null
  gitDir: string
}

export interface FolderLabel {
  label: string
  headPath: string | null
}

const GIT_ENTRY = '.git'
const GITDIR_PREFIX = 'gitdir:'
const WORKTREES_DIR = 'worktrees'
const BRANCH_PREFIX = 'ref: refs/heads/'
const MAIN_BRANCHES: ReadonlySet<string> = new Set(['main', 'master'])

const entryKind = async (
  path: string,
): Promise<'directory' | 'file' | null> => {
  try {
    const stats = await stat(path)
    if (stats.isDirectory()) {
      return 'directory'
    }
    return stats.isFile() ? 'file' : null
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code
    if (code === 'ENOENT' || code === 'ENOTDIR') {
      return null
    }
    throw error
  }
}

const nameOf = (root: string): string | null => {
  const name = basename(root)
  if (name === '') {
    return null
  }
  const trimmed = name.endsWith('.git') ? name.slice(0, -'.git'.length) : name
  return trimmed === '' ? name : trimmed
}

const identity = (
  root: string,
  worktree: string | null,
  gitDir: string,
): RepoIdentity | null => {
  const name = nameOf(root)
  return name === null ? null : { root, name, worktree, gitDir }
}

const linkedWorktree = (
  gitDir: string,
): { root: string; worktree: string } | null => {
  const worktrees = dirname(gitDir)
  if (worktrees === gitDir || basename(worktrees) !== WORKTREES_DIR) {
    return null
  }
  const gitDirectory = dirname(worktrees)
  const root =
    basename(gitDirectory) === GIT_ENTRY ? dirname(gitDirectory) : gitDirectory
  return { root, worktree: basename(gitDir) }
}

const fromPointer = async (
  directory: string,
  pointer: string,
): Promise<RepoIdentity | null> => {
  let contents: string
  try {
    contents = await readFile(pointer, 'utf8')
  } catch {
    return null
  }
  const line = contents
    .split('\n')
    .map((text) => text.trim())
    .find((text) => text.startsWith(GITDIR_PREFIX))
  if (line === undefined) {
    return null
  }
  const gitDir = resolve(directory, line.slice(GITDIR_PREFIX.length).trim())
  const linked = linkedWorktree(gitDir)
  return linked === null
    ? identity(directory, null, gitDir)
    : identity(linked.root, linked.worktree, gitDir)
}

export const identifyRepo = async (
  folder: string,
): Promise<RepoIdentity | null> => {
  let directory = resolve(folder)
  for (;;) {
    const entry = join(directory, GIT_ENTRY)
    const kind = await entryKind(entry)
    if (kind === 'directory') {
      return identity(directory, null, entry)
    }
    if (kind === 'file') {
      return fromPointer(directory, entry)
    }
    const parent = dirname(directory)
    if (parent === directory) {
      return null
    }
    directory = parent
  }
}

export const readBranch = async (gitDir: string): Promise<string | null> => {
  let head: string
  try {
    head = await readFile(join(gitDir, 'HEAD'), 'utf8')
  } catch {
    return null
  }
  const ref = head.trim()
  if (!ref.startsWith(BRANCH_PREFIX)) {
    return null
  }
  const branch = ref.slice(BRANCH_PREFIX.length).trim()
  return branch === '' ? null : branch
}

export const repoLabel = (
  identity: RepoIdentity | null,
  branch: string | null,
  folder: string,
): string => {
  if (identity === null) {
    return basename(folder)
  }
  if (identity.worktree !== null) {
    return `${identity.name} · ${identity.worktree}`
  }
  if (branch === null || MAIN_BRANCHES.has(branch)) {
    return identity.name
  }
  return `${identity.name} · ${branch}`
}

export const folderLabel = async (folder: string): Promise<FolderLabel> => {
  try {
    const repo = await identifyRepo(folder)
    if (repo === null || repo.worktree !== null) {
      return { label: repoLabel(repo, null, folder), headPath: null }
    }
    return {
      label: repoLabel(repo, await readBranch(repo.gitDir), folder),
      headPath: join(repo.gitDir, 'HEAD'),
    }
  } catch {
    return { label: basename(folder), headPath: null }
  }
}
