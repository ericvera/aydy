import { mkdir, readFile } from 'node:fs/promises'
import { join } from 'node:path'

import { ChangedOnDisk, writeAtomically, writeIfUnchanged } from './atomicWrite'
import type { PathOptions } from './aydyPaths'
import {
  binDir,
  claudeSettingsPath,
  HOOK_SCRIPT_SOURCE,
  hookScriptPath,
} from './aydyPaths'
import {
  hasAllHooks,
  UnexpectedShape,
  withAydyHooks,
  withoutAydyHooks,
} from './hookSettings'

const SCRIPT_MODE = 0o755

const DEFAULT_INDENT = '  '

export type HookFailure =
  'malformed' | 'unexpected' | 'unreadable' | 'unwritable' | 'changedOnDisk'

export type HookResult =
  { ok: true; changed: boolean } | { ok: false; reason: HookFailure }

type Json = Record<string, unknown>

type Transform = (doc: Json) => Json | null

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

export const installHookScript = async (
  extensionPath: string,
  paths?: PathOptions,
): Promise<boolean> => {
  const source = await readFile(join(extensionPath, HOOK_SCRIPT_SOURCE), 'utf8')
  const target = hookScriptPath(paths)
  if ((await readOrNull(target)) === source) {
    return false
  }
  await mkdir(binDir(paths), { recursive: true })
  await writeAtomically(target, source, SCRIPT_MODE)
  return true
}

const detectIndent = (raw: string): string => {
  for (const line of raw.split('\n')) {
    const content = line.replace(/^[ \t]+/, '')
    if (content !== line && content.trim() !== '') {
      return line.slice(0, line.length - content.length)
    }
  }
  return DEFAULT_INDENT
}

const render = (doc: Json, raw: string | null): string => {
  const text = JSON.stringify(
    doc,
    null,
    raw === null ? DEFAULT_INDENT : detectIndent(raw),
  )
  return raw === null || raw.endsWith('\n') ? `${text}\n` : text
}

const isRecord = (value: unknown): value is Json =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const attempt = async (
  path: string,
  transform: Transform,
  missingAsEmpty: boolean,
): Promise<HookResult> => {
  let raw: string | null
  try {
    raw = await readOrNull(path)
  } catch {
    return { ok: false, reason: 'unreadable' }
  }
  if (raw === null && !missingAsEmpty) {
    return { ok: true, changed: false }
  }
  let doc: unknown
  try {
    doc = raw === null ? {} : JSON.parse(raw)
  } catch {
    return { ok: false, reason: 'malformed' }
  }
  if (!isRecord(doc)) {
    return { ok: false, reason: 'unexpected' }
  }
  let next: Json | null
  try {
    next = transform(doc)
  } catch (error) {
    if (error instanceof UnexpectedShape) {
      return { ok: false, reason: 'unexpected' }
    }
    throw error
  }
  if (next === null) {
    return { ok: true, changed: false }
  }
  try {
    await writeIfUnchanged(path, raw, render(next, raw))
  } catch (error) {
    if (error instanceof ChangedOnDisk) {
      return { ok: false, reason: 'changedOnDisk' }
    }
    return { ok: false, reason: 'unwritable' }
  }
  return { ok: true, changed: true }
}

const withRetry = async (
  path: string,
  transform: Transform,
  missingAsEmpty: boolean,
): Promise<HookResult> => {
  const first = await attempt(path, transform, missingAsEmpty)
  return !first.ok && first.reason === 'changedOnDisk'
    ? attempt(path, transform, missingAsEmpty)
    : first
}

export const ensureHooks = (paths?: PathOptions): Promise<HookResult> =>
  withRetry(
    claudeSettingsPath(paths),
    (doc) => (hasAllHooks(doc) ? null : withAydyHooks(doc)),
    true,
  )

export const removeHooks = (paths?: PathOptions): Promise<HookResult> =>
  withRetry(
    claudeSettingsPath(paths),
    (doc) => {
      const next = withoutAydyHooks(doc)
      return JSON.stringify(next) === JSON.stringify(doc) ? null : next
    },
    false,
  )
