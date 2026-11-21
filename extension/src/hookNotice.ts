import { mkdir, unlink, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { join, sep } from 'node:path'

import { noticesDir } from './aydyPaths'
import type { HookFailure } from './hookInstall'

const MARKER = 'hook-install-failed'

const DETAIL: Record<HookFailure, string> = {
  malformed: 'The file is not valid JSON.',
  unexpected: 'Its hooks section is not in the expected shape.',
  unreadable: 'The file could not be read.',
  unwritable: 'The file could not be written.',
  changedOnDisk: 'It kept changing while AYDY was writing.',
}

const errorCode = (error: unknown): string | undefined =>
  error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined

export const claimFailureNotice = async (
  dir: string = noticesDir(),
): Promise<boolean> => {
  await mkdir(dir, { recursive: true })
  try {
    await writeFile(join(dir, MARKER), '', { flag: 'wx' })
    return true
  } catch (error) {
    if (errorCode(error) === 'EEXIST') {
      return false
    }
    throw error
  }
}

export const releaseFailureNotice = async (
  dir: string = noticesDir(),
): Promise<void> => {
  try {
    await unlink(join(dir, MARKER))
  } catch (error) {
    if (errorCode(error) !== 'ENOENT') {
      throw error
    }
  }
}

const tildePath = (path: string, home: string): string =>
  path.startsWith(home + sep) ? `~${path.slice(home.length)}` : path

export const failureMessage = (
  reason: HookFailure,
  settingsPathForDisplay: string,
  home: string = homedir(),
): string =>
  `AYDY could not add its hooks to ${tildePath(settingsPathForDisplay, home)}, so sessions are not being tracked. ${DETAIL[reason]} Nothing in it was changed.`
