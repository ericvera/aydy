import { createHash } from 'node:crypto'
import { mkdir, readFile, realpath } from 'node:fs/promises'
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

export const generatedWorkspacePath = (
  folderRealPath: string,
  dirs: WorkspaceDirs = defaultDirs(),
): string => {
  const hash = createHash('sha256')
    .update(folderRealPath)
    .digest('hex')
    .slice(0, HASH_LENGTH)
  return join(
    dirs.workspaces,
    hash,
    `${basename(folderRealPath)}.code-workspace`,
  )
}

export const isGeneratedWorkspace = (
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

export const writeWorkspace = async (
  folder: string,
  title: string | null,
  dirs: WorkspaceDirs = defaultDirs(),
): Promise<string> => {
  const path = generatedWorkspacePath(await realpath(folder), dirs)
  await mkdir(dirname(path), { recursive: true })
  await rewrite(path, folder, title)
  return path
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
