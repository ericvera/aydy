import { randomBytes } from 'node:crypto'
import { watch } from 'node:fs'
import type { FSWatcher } from 'node:fs'
import { mkdir, readdir, readFile, rename, rm } from 'node:fs/promises'
import { join } from 'node:path'

import { writeAtomically } from './atomicWrite'
import type { RevealOutcome, RevealRequest } from './revealSession'
import { containingFolderLength, ownerOf } from './sessionState'
import type { WindowFolders } from './sessionState'
import { readLiveSessions, resolveCwd } from './sessionStore'
import { knownWindows } from './windowRegistry'

export interface JumpRequest {
  id: string
  sessionId: string
  cwd: string
  tabLabel: string | null
  requestedAt: number
}

export interface JumpResponderOptions {
  dir: string
  pid: number
  folders: () => string[]
  windows?: () => Promise<WindowFolders[]>
  launchCwds?: () => Promise<ReadonlyMap<string, string | null> | null>
  resolveCwd?: (cwd: string | null) => Promise<string | null>
  open: () => string | null
  reveal: (request: RevealRequest) => Promise<RevealOutcome>
  now?: () => number
  pollMs?: number
}

const JUMP_ID_PATTERN = /^[A-Za-z0-9_.-]{1,128}$/

const STALE_AFTER_MS = 10_000

/** The app's 3 s reply timeout plus its 1 s grace after a late claim. */
export const REPLY_BUDGET_MS = 4_000

const REPLY_SUFFIX = '.reply.json'

const errorCode = (error: unknown): string | undefined =>
  error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined

export const parseJumpRequest = (text: string): JumpRequest | null => {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    return null
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return null
  }
  const { id, sessionId, cwd, tabLabel, requestedAt } = value as Record<
    string,
    unknown
  >
  if (
    typeof id !== 'string' ||
    !JUMP_ID_PATTERN.test(id) ||
    typeof sessionId !== 'string' ||
    typeof cwd !== 'string' ||
    (tabLabel !== null && typeof tabLabel !== 'string') ||
    typeof requestedAt !== 'number' ||
    !Number.isFinite(requestedAt)
  ) {
    return null
  }
  return { id, sessionId, cwd, tabLabel, requestedAt }
}

export const shouldHandle = (
  request: JumpRequest,
  selfPid: number,
  windows: WindowFolders[],
  now: number,
): boolean =>
  ownerOf(request.cwd, windows) === selfPid &&
  now - request.requestedAt <= STALE_AFTER_MS

const owningFolder = (cwd: string, folders: string[]): string | null => {
  const length = containingFolderLength(cwd, folders)
  if (length < 0) {
    return null
  }
  return (
    folders.find(
      (folder) => containingFolderLength(cwd, [folder]) === length,
    ) ?? null
  )
}

export class JumpResponder {
  private readonly dir: string
  private readonly pid: number
  private readonly folders: () => string[]
  private readonly windows: () => Promise<WindowFolders[]>
  private readonly launchCwds: () => Promise<ReadonlyMap<
    string,
    string | null
  > | null>
  private readonly resolveCwd: (cwd: string | null) => Promise<string | null>
  private readonly open: () => string | null
  private readonly reveal: (request: RevealRequest) => Promise<RevealOutcome>
  private readonly now: () => number
  private readonly pollMs: number

  private readonly handled = new Set<string>()
  private watcher: FSWatcher | null = null
  private poll: NodeJS.Timeout | null = null
  private running: Promise<void> | null = null
  private rerun = false
  private disposed = false

  constructor(options: JumpResponderOptions) {
    this.dir = options.dir
    this.pid = options.pid
    this.folders = options.folders
    this.windows =
      options.windows ??
      (async () => knownWindows(options.pid, options.folders()))
    this.launchCwds = options.launchCwds ?? (async () => readLiveSessions())
    this.resolveCwd = options.resolveCwd ?? resolveCwd
    this.open = options.open
    this.reveal = options.reveal
    this.now = options.now ?? Date.now
    this.pollMs = options.pollMs ?? 1000
  }

  async start(): Promise<void> {
    try {
      await mkdir(this.dir, { recursive: true })
    } catch {
      // Polling picks the directory up once it can be made.
    }
    if (this.disposed) {
      return
    }
    try {
      const watcher = watch(this.dir, () => {
        void this.scan()
      })
      watcher.on('error', () => undefined)
      this.watcher = watcher
    } catch {
      // Polling covers a directory that cannot be watched.
    }
    this.poll = setInterval(() => {
      void this.scan()
    }, this.pollMs)
    this.poll.unref()
    await this.scan()
  }

  /** Runs one scan, or queues another after the one in flight. */
  async scan(): Promise<void> {
    if (this.disposed) {
      return
    }
    if (this.running !== null) {
      this.rerun = true
      return this.running
    }
    this.running = this.drain()
    try {
      await this.running
    } finally {
      this.running = null
    }
  }

  dispose(): void {
    this.disposed = true
    if (this.poll !== null) {
      clearInterval(this.poll)
      this.poll = null
    }
    this.watcher?.close()
    this.watcher = null
  }

  private async drain(): Promise<void> {
    do {
      this.rerun = false
      try {
        await this.pass()
      } catch {
        // IO errors wait for the next scan.
      }
    } while (this.rerun && !this.disposed)
  }

  private async pass(): Promise<void> {
    const names = (await readdir(this.dir)).filter(
      (name) =>
        !name.startsWith('.') &&
        name.endsWith('.json') &&
        !name.endsWith(REPLY_SUFFIX) &&
        !this.handled.has(name.slice(0, -'.json'.length)),
    )
    if (names.length === 0) {
      return
    }
    const [windows, launch] = await Promise.all([
      this.windows(),
      this.launchCwds(),
    ])
    for (const name of names) {
      if (this.disposed) {
        return
      }
      try {
        await this.consider(name, windows, launch)
      } catch {
        // One unreadable request must not hold up the rest.
      }
    }
  }

  private async consider(
    name: string,
    windows: WindowFolders[],
    launch: ReadonlyMap<string, string | null> | null,
  ): Promise<void> {
    const path = join(this.dir, name)
    let text: string
    try {
      text = await readFile(path, 'utf8')
    } catch (error) {
      if (errorCode(error) === 'ENOENT') {
        return
      }
      throw error
    }
    const parsed = parseJumpRequest(text)
    if (parsed === null || `${parsed.id}.json` !== name) {
      return
    }
    const cwd = await this.resolveCwd(
      launch?.get(parsed.sessionId) ?? parsed.cwd,
    )
    if (cwd === null) {
      return
    }
    const request = { ...parsed, cwd }
    if (!shouldHandle(request, this.pid, windows, this.now())) {
      return
    }
    // A rename claims the request: two windows unlinking it at once can both
    // succeed on APFS, while only one rename of the same file can.
    const claimed = join(
      this.dir,
      `.${request.id}.${this.pid}.${randomBytes(6).toString('hex')}.claimed`,
    )
    try {
      await rename(path, claimed)
    } catch (error) {
      if (errorCode(error) === 'ENOENT') {
        this.handled.add(request.id)
        return
      }
      throw error
    }
    this.handled.add(request.id)
    await rm(claimed, { force: true })
    const reveal: RevealOutcome =
      request.tabLabel === null
        ? 'notPresent'
        : await this.reveal({
            sessionId: request.sessionId,
            tabLabel: request.tabLabel,
          })
    if (this.now() - request.requestedAt > REPLY_BUDGET_MS) {
      return
    }
    const open = this.open() ?? owningFolder(cwd, this.folders())
    if (open === null) {
      return
    }
    await writeAtomically(
      join(this.dir, `${request.id}${REPLY_SUFFIX}`),
      JSON.stringify({ open, reveal }),
    )
  }
}
