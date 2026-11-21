import { createHash } from 'node:crypto'
import { mkdir, readdir, readFile, rm, stat } from 'node:fs/promises'
import { basename, dirname, isAbsolute, join, relative } from 'node:path'

import { writeAtomically } from './atomicWrite'
import { workspacesDir } from './aydyPaths'
import { parseJsonc } from './jsonc'

export type WorkspaceDirs = { workspaces: string }

type Json = Record<string, unknown>

const TITLE_KEY = 'window.title'

const HASH_LENGTH = 12

const defaultDirs = (): WorkspaceDirs => ({ workspaces: workspacesDir() })

const isPlainObject = (value: unknown): value is Json =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const errorCode = (error: unknown): string | undefined =>
  error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined

const readOrNull = async (path: string): Promise<string | null> => {
  try {
    return await readFile(path, 'utf8')
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return null
    }
    throw error
  }
}

const parseObjectOrNull = (text: string): Json | null => {
  try {
    const parsed = parseJsonc(text)
    return isPlainObject(parsed) ? parsed : null
  } catch {
    return null
  }
}

const shortHash = (text: string): string =>
  createHash('sha256').update(text).digest('hex').slice(0, HASH_LENGTH)

/**
 * Where AYDY keeps a folder's workspace file: right beside the folder, named
 * after it, so `~/Code/okven` gets `~/Code/okven.code-workspace` and it can be
 * found in Finder and in Open Recent by name.
 */
export const siblingWorkspacePath = (folderRealPath: string): string =>
  join(dirname(folderRealPath), `${basename(folderRealPath)}.code-workspace`)

/**
 * Where AYDY used to keep every workspace file, and still does for a folder
 * whose sibling path is taken by a file you declined to overwrite.
 */
export const legacyWorkspacePath = (
  folderRealPath: string,
  dirs: WorkspaceDirs = defaultDirs(),
): string =>
  join(
    dirs.workspaces,
    shortHash(folderRealPath),
    `${basename(folderRealPath)}.code-workspace`,
  )

export const isLegacyWorkspace = (
  fsPath: string,
  dirs: WorkspaceDirs = defaultDirs(),
): boolean => {
  const rel = relative(dirs.workspaces, fsPath)
  return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel)
}

export const jsonEqual = (a: unknown, b: unknown): boolean => {
  if (Array.isArray(a) || Array.isArray(b)) {
    return (
      Array.isArray(a) &&
      Array.isArray(b) &&
      a.length === b.length &&
      a.every((item, index) => jsonEqual(item, b[index]))
    )
  }
  if (isPlainObject(a) && isPlainObject(b)) {
    const keys = Object.keys(a)
    return (
      keys.length === Object.keys(b).length &&
      keys.every((key) => Object.hasOwn(b, key) && jsonEqual(a[key], b[key]))
    )
  }
  return a === b
}

const hasStringPath = (entry: unknown): entry is Json & { path: string } =>
  isPlainObject(entry) && typeof entry['path'] === 'string'

const mergeFolders = (existing: unknown, folder: string): Json[] => {
  const entries = Array.isArray(existing) ? existing.filter(hasStringPath) : []
  const primary = entries.find((entry) => entry.path === folder) ?? {
    path: folder,
  }
  return [primary, ...entries.filter((entry) => entry.path !== folder)]
}

const previousCopied = (existing: Json | null): Json => {
  const aydy = existing?.['aydy']
  const copied = isPlainObject(aydy) ? aydy['copiedSettings'] : undefined
  return isPlainObject(copied) ? copied : {}
}

export const buildWorkspace = (
  existing: Json | null,
  folder: string,
  folderSettings: Json,
  title: string | null,
): Json => {
  const base = existing ?? {}
  const settings: Json = isPlainObject(base['settings'])
    ? { ...base['settings'] }
    : {}
  const prev = previousCopied(existing)
  for (const [key, value] of Object.entries(prev)) {
    if (jsonEqual(settings[key], value)) {
      delete settings[key]
    }
  }
  const copied: Json = {}
  for (const [key, value] of Object.entries(folderSettings)) {
    if (key === TITLE_KEY || key in settings) {
      continue
    }
    settings[key] = value
    copied[key] = value
  }
  if (title !== null) {
    settings[TITLE_KEY] = title
  }
  return {
    ...base,
    folders: mergeFolders(base['folders'], folder),
    settings,
    aydy: { folder, copiedSettings: copied },
  }
}

export const readFolderSettings = async (
  folder: string,
): Promise<Json | null> => {
  const text = await readOrNull(join(folder, '.vscode', 'settings.json'))
  return text === null ? {} : parseObjectOrNull(text)
}

const readExisting = async (path: string): Promise<Json | null> => {
  const text = await readOrNull(path)
  return text === null ? null : parseObjectOrNull(text)
}

const render = (doc: Json): string => `${JSON.stringify(doc, null, 2)}\n`

const rewrite = async (
  path: string,
  folder: string,
  title: string | null,
): Promise<void> => {
  const existing = await readExisting(path)
  const folderSettings =
    (await readFolderSettings(folder)) ?? previousCopied(existing)
  await writeAtomically(
    path,
    render(buildWorkspace(existing, folder, folderSettings, title)),
  )
}

/**
 * Writes the workspace file for `folder` at `path`, merging into what AYDY
 * wrote there before. `fresh` discards whatever is at `path` instead — used
 * only once the user agreed to overwrite a file AYDY did not write.
 */
export const writeWorkspace = async (
  path: string,
  folder: string,
  title: string | null,
  { fresh = false }: { fresh?: boolean } = {},
): Promise<void> => {
  await mkdir(dirname(path), { recursive: true })
  if (fresh) {
    await writeAtomically(
      path,
      render(
        buildWorkspace(
          null,
          folder,
          (await readFolderSettings(folder)) ?? {},
          title,
        ),
      ),
    )
    return
  }
  await rewrite(path, folder, title)
}

export const setWorkspaceTitle = async (
  path: string,
  title: string,
): Promise<boolean> => {
  const existing = await readExisting(path)
  if (existing === null) {
    return false
  }
  const settings = isPlainObject(existing['settings'])
    ? existing['settings']
    : {}
  if (settings[TITLE_KEY] === title) {
    return false
  }
  await writeAtomically(
    path,
    render({ ...existing, settings: { ...settings, [TITLE_KEY]: title } }),
  )
  return true
}

export const readWorkspaceFolder = async (
  path: string,
): Promise<string | null> => {
  const aydy = (await readExisting(path))?.['aydy']
  const folder = isPlainObject(aydy) ? aydy['folder'] : undefined
  return typeof folder === 'string' ? folder : null
}

export const syncFolderSettings = async (path: string): Promise<void> => {
  const folder = await readWorkspaceFolder(path)
  if (folder !== null) {
    await rewrite(path, folder, null)
  }
}

export type SiblingState = 'free' | 'ours' | 'foreign'

/**
 * Whether the file at `path` is missing, one AYDY wrote for `folder`, or
 * anything else — a workspace of your own, or one AYDY wrote for a folder that
 * has since moved.
 */
export const workspaceState = async (
  path: string,
  folder: string,
): Promise<SiblingState> => {
  if ((await readOrNull(path)) === null) {
    return 'free'
  }
  return (await readWorkspaceFolder(path)) === folder ? 'ours' : 'foreign'
}

const indexDir = (dirs: WorkspaceDirs): string => join(dirs.workspaces, 'index')

/**
 * Records a workspace file AYDY wrote, so it can be found and removed once its
 * folder is gone, wherever it lives.
 */
export const registerWorkspace = async (
  path: string,
  folder: string,
  dirs: WorkspaceDirs = defaultDirs(),
): Promise<void> => {
  const dir = indexDir(dirs)
  await mkdir(dir, { recursive: true })
  await writeAtomically(
    join(dir, `${shortHash(path)}.json`),
    render({ workspace: path, folder }),
  )
}

const exists = async (path: string): Promise<boolean> => {
  try {
    await stat(path)
    return true
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return false
    }
    throw error
  }
}

/**
 * Whether `folder` is gone for good: it is missing while the directory it
 * lived in is still there. An unmounted volume takes the parent with it, so
 * its folders are never taken for deleted.
 */
const folderGone = async (folder: string): Promise<boolean> =>
  !(await exists(folder)) && (await exists(dirname(folder)))

const listOrEmpty = async (dir: string): Promise<string[]> => {
  try {
    return await readdir(dir)
  } catch (error) {
    if (errorCode(error) === 'ENOENT' || errorCode(error) === 'ENOTDIR') {
      return []
    }
    throw error
  }
}

const sweepIndex = async (dirs: WorkspaceDirs): Promise<string[]> => {
  const removed: string[] = []
  const dir = indexDir(dirs)
  for (const name of await listOrEmpty(dir)) {
    if (!name.endsWith('.json')) {
      continue
    }
    const entryPath = join(dir, name)
    const entry = parseObjectOrNull((await readOrNull(entryPath)) ?? '')
    const workspace = entry?.['workspace']
    const folder = entry?.['folder']
    if (typeof workspace !== 'string' || typeof folder !== 'string') {
      await rm(entryPath, { force: true })
      continue
    }
    const state = await workspaceState(workspace, folder)
    if (state !== 'ours') {
      await rm(entryPath, { force: true })
      continue
    }
    if (await folderGone(folder)) {
      await rm(workspace, { force: true })
      await rm(entryPath, { force: true })
      removed.push(workspace)
    }
  }
  return removed
}

const sweepLegacy = async (dirs: WorkspaceDirs): Promise<string[]> => {
  const removed: string[] = []
  for (const hash of await listOrEmpty(dirs.workspaces)) {
    if (!/^[0-9a-f]{12}$/.test(hash)) {
      continue
    }
    const dir = join(dirs.workspaces, hash)
    for (const name of await listOrEmpty(dir)) {
      if (!name.endsWith('.code-workspace')) {
        continue
      }
      const path = join(dir, name)
      const folder = await readWorkspaceFolder(path)
      if (folder !== null && (await folderGone(folder))) {
        await rm(dir, { force: true, recursive: true })
        removed.push(path)
        break
      }
    }
  }
  return removed
}

/**
 * Removes the workspace files AYDY wrote for folders that no longer exist —
 * a deleted worktree's `feat-x.code-workspace`, say. Only files that still
 * carry AYDY's record for that same folder are touched.
 */
export const sweepWorkspaces = async (
  dirs: WorkspaceDirs = defaultDirs(),
): Promise<string[]> => [
  ...(await sweepIndex(dirs)),
  ...(await sweepLegacy(dirs)),
]

/** Removes `folder`'s legacy workspace file once it has moved beside it. */
export const removeLegacyWorkspace = async (
  folder: string,
  dirs: WorkspaceDirs = defaultDirs(),
): Promise<void> => {
  const path = legacyWorkspacePath(folder, dirs)
  if ((await workspaceState(path, folder)) === 'ours') {
    await rm(dirname(path), { force: true, recursive: true })
  }
}
