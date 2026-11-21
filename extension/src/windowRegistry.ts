import { mkdir, readdir, readFile, unlink } from 'node:fs/promises'
import { join } from 'node:path'

import { writeAtomically } from './atomicWrite'
import { windowsDir } from './aydyPaths'
import { isAlive as processIsAlive } from './sessionStore'
import type { WindowFolders } from './sessionState'

export interface RegistryOptions {
  dir?: string
  isAlive?: (pid: number) => boolean
}

const WINDOW_FILE = /^(\d+)\.json$/

const errorCode = (error: unknown): string | undefined =>
  error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined

const parseWindow = (text: string, pid: number): WindowFolders | null => {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    return null
  }
  if (typeof value !== 'object' || value === null) {
    return null
  }
  const record = value as Record<string, unknown>
  const folders = record['folders']
  if (
    record['pid'] !== pid ||
    !Array.isArray(folders) ||
    !folders.every((folder) => typeof folder === 'string')
  ) {
    return null
  }
  return { pid, folders }
}

export const publishWindow = async (
  pid: number,
  folders: string[],
  { dir = windowsDir() }: RegistryOptions = {},
): Promise<void> => {
  await mkdir(dir, { recursive: true })
  await writeAtomically(
    join(dir, `${pid}.json`),
    JSON.stringify({ pid, folders }),
  )
}

export const unpublishWindow = async (
  pid: number,
  { dir = windowsDir() }: RegistryOptions = {},
): Promise<void> => {
  try {
    await unlink(join(dir, `${pid}.json`))
  } catch (error) {
    if (errorCode(error) !== 'ENOENT') {
      throw error
    }
  }
}

export const readWindows = async ({
  dir = windowsDir(),
  isAlive = processIsAlive,
}: RegistryOptions = {}): Promise<WindowFolders[]> => {
  let names: string[]
  try {
    names = await readdir(dir)
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return []
    }
    throw error
  }
  const windows: WindowFolders[] = []
  for (const name of names) {
    const match = WINDOW_FILE.exec(name)
    if (match === null) {
      continue
    }
    const path = join(dir, name)
    let text: string
    try {
      text = await readFile(path, 'utf8')
    } catch {
      continue
    }
    const entry = parseWindow(text, Number(match[1]))
    if (entry === null) {
      continue
    }
    if (!isAlive(entry.pid)) {
      await unlink(path).catch(() => undefined)
      continue
    }
    windows.push(entry)
  }
  return windows
}

export const knownWindows = async (
  selfPid: number,
  selfFolders: string[],
  options: RegistryOptions = {},
): Promise<WindowFolders[]> => [
  ...(await readWindows(options)).filter(({ pid }) => pid !== selfPid),
  { pid: selfPid, folders: selfFolders },
]
