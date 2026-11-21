import { randomBytes } from 'node:crypto'
import { readFileSync, renameSync } from 'node:fs'
import {
  lstat,
  mkdir,
  open,
  readlink,
  realpath,
  rename,
  rm,
  stat,
  chmod,
} from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'

const MAX_LINK_HOPS = 32

export class ChangedOnDisk extends Error {
  constructor(path: string) {
    super(`${path} changed on disk before it could be written`)
    this.name = 'ChangedOnDisk'
  }
}

export class TooManyLinks extends Error {
  constructor(path: string) {
    super(`${path} has more than ${MAX_LINK_HOPS} levels of symbolic links`)
    this.name = 'TooManyLinks'
  }
}

const errorCode = (error: unknown): string | undefined =>
  error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined

const isLink = async (path: string): Promise<boolean> => {
  try {
    return (await lstat(path)).isSymbolicLink()
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return false
    }
    throw error
  }
}

export const resolveTarget = async (path: string): Promise<string> => {
  if (!(await isLink(path))) {
    return path
  }
  try {
    return await realpath(path)
  } catch (error) {
    if (errorCode(error) !== 'ENOENT') {
      throw error
    }
  }
  let current = path
  for (let hop = 0; hop < MAX_LINK_HOPS; hop += 1) {
    if (!(await isLink(current))) {
      return current
    }
    current = resolve(dirname(current), await readlink(current))
  }
  throw new TooManyLinks(path)
}

const readOrNull = (path: string): string | null => {
  try {
    return readFileSync(path, 'utf8')
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return null
    }
    throw error
  }
}

const existingMode = async (path: string): Promise<number | undefined> => {
  try {
    return (await stat(path)).mode & 0o7777
  } catch (error) {
    if (errorCode(error) === 'ENOENT') {
      return undefined
    }
    throw error
  }
}

const writeTemporary = async (
  real: string,
  text: string,
  mode: number | undefined,
): Promise<string> => {
  const directory = dirname(real)
  await mkdir(directory, { recursive: true })
  const temporary = join(
    directory,
    `.${basename(real)}.${process.pid}.${randomBytes(6).toString('hex')}.tmp`,
  )
  try {
    const handle = await open(temporary, 'wx')
    try {
      await handle.writeFile(text, 'utf8')
      await handle.sync()
    } finally {
      await handle.close()
    }
    const carried = mode ?? (await existingMode(real))
    if (carried !== undefined) {
      await chmod(temporary, carried)
    }
    return temporary
  } catch (error) {
    await rm(temporary, { force: true })
    throw error
  }
}

export const writeAtomically = async (
  path: string,
  text: string,
  mode?: number,
): Promise<void> => {
  const real = await resolveTarget(path)
  const temporary = await writeTemporary(real, text, mode)
  try {
    await rename(temporary, real)
  } catch (error) {
    await rm(temporary, { force: true })
    throw error
  }
}

export const writeIfUnchanged = async (
  path: string,
  expected: string | null,
  text: string,
): Promise<void> => {
  const real = await resolveTarget(path)
  const temporary = await writeTemporary(real, text, undefined)
  try {
    // Synchronous so no other write in this process can land between the
    // check and the rename.
    if (readOrNull(real) !== expected) {
      throw new ChangedOnDisk(path)
    }
    renameSync(temporary, real)
  } catch (error) {
    await rm(temporary, { force: true })
    throw error
  }
}
