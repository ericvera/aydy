import { watch } from 'node:fs'
import { mkdir } from 'node:fs/promises'

import { claudeSessionsDir, seenDir, statusDir, windowsDir } from './aydyPaths'
import { isUnread, ownerOf, sessionDir, windowGlance } from './sessionState'
import type { SessionStatus, WindowFolders, WindowGlance } from './sessionState'
import {
  pruneStale,
  readLiveSessions,
  readSeen,
  readStatuses,
  resolveCwd,
  writeSeen,
} from './sessionStore'
import { knownWindows } from './windowRegistry'

export interface TrackerDirs {
  status: string
  seen: string
  windows: string
  sessions: string
}

export interface TrackerStore {
  readStatuses: () => Promise<SessionStatus[]>
  readSeen: () => Promise<Map<string, number>>
  writeSeen: (sessionId: string, ms: number) => Promise<void>
  readLiveSessions: () => Promise<Map<string, string | null> | null>
  resolveCwd: (cwd: string | null) => Promise<string | null>
  pruneStale: (
    live: ReadonlyMap<string, unknown> | null,
    now: number,
  ) => Promise<void>
  knownWindows: (
    selfPid: number,
    selfFolders: string[],
  ) => Promise<WindowFolders[]>
}

export interface WindowTrackerOptions {
  pid: number
  folders: () => string[]
  focused: () => boolean
  onGlance: (glance: WindowGlance) => void
  dirs?: TrackerDirs
  store?: TrackerStore
  debounceMs?: number
  pollMs?: number
  /** Watches a directory, calling `onChange` on any change in it. */
  watchDir?: WatchDir
}

export type WatchDir = (
  dir: string,
  onChange: () => void,
) => { close: () => void }

const watchWithFs: WatchDir = (dir, onChange) => {
  const watcher = watch(dir, onChange)
  watcher.on('error', () => undefined)
  return watcher
}

export const TRACKER_DEBOUNCE_MS = 150
export const TRACKER_POLL_MS = 5000

const defaultDirs = (): TrackerDirs => ({
  status: statusDir(),
  seen: seenDir(),
  windows: windowsDir(),
  sessions: claudeSessionsDir(),
})

const defaultStore = (dirs: TrackerDirs): TrackerStore => ({
  readStatuses: async () => readStatuses(dirs),
  readSeen: async () => readSeen(dirs),
  writeSeen: async (sessionId, ms) => writeSeen(sessionId, ms, dirs),
  readLiveSessions: async () => readLiveSessions(dirs),
  resolveCwd,
  pruneStale: async (live, now) => pruneStale(live, now, dirs),
  knownWindows: async (selfPid, selfFolders) =>
    knownWindows(selfPid, selfFolders, { dir: dirs.windows }),
})

export class WindowTracker {
  private readonly pid: number
  private readonly folders: () => string[]
  private readonly focused: () => boolean
  private readonly onGlance: (glance: WindowGlance) => void
  private readonly dirs: TrackerDirs
  private readonly store: TrackerStore
  private readonly debounceMs: number
  private readonly pollMs: number
  private readonly watchDir: WatchDir

  private watchers: { close: () => void }[] = []
  private timer: NodeJS.Timeout | null = null
  private poll: NodeJS.Timeout | null = null
  private running = false
  private rerun = false
  private disposed = false
  private lastGlance: WindowGlance | null = null

  constructor(options: WindowTrackerOptions) {
    this.pid = options.pid
    this.folders = options.folders
    this.focused = options.focused
    this.onGlance = options.onGlance
    this.dirs = options.dirs ?? defaultDirs()
    this.store = options.store ?? defaultStore(this.dirs)
    this.debounceMs = options.debounceMs ?? TRACKER_DEBOUNCE_MS
    this.pollMs = options.pollMs ?? TRACKER_POLL_MS
    this.watchDir = options.watchDir ?? watchWithFs
  }

  async start(): Promise<void> {
    const watched = [this.dirs.status, this.dirs.seen, this.dirs.windows]
    for (const dir of watched) {
      try {
        await mkdir(dir, { recursive: true })
      } catch {
        // Polling and the next pass cover a directory that cannot be made yet.
      }
    }
    if (this.disposed) {
      return
    }
    for (const dir of watched) {
      try {
        this.watchers.push(
          this.watchDir(dir, () => {
            this.poke()
          }),
        )
      } catch {
        // Polling covers a directory that cannot be watched.
      }
    }
    this.poll = setInterval(() => {
      this.poke()
    }, this.pollMs)
    this.poll.unref()
    try {
      await this.store.pruneStale(
        await this.store.readLiveSessions(),
        Date.now(),
      )
    } catch {
      // Pruning is housekeeping; a failure leaves files for a later start.
    }
    this.poke()
  }

  poke(): void {
    if (this.disposed) {
      return
    }
    if (this.timer !== null) {
      clearTimeout(this.timer)
    }
    this.timer = setTimeout(() => {
      this.timer = null
      void this.runPass()
    }, this.debounceMs)
    this.timer.unref()
  }

  dispose(): void {
    this.disposed = true
    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }
    if (this.poll !== null) {
      clearInterval(this.poll)
      this.poll = null
    }
    for (const watcher of this.watchers) {
      watcher.close()
    }
    this.watchers = []
  }

  private async runPass(): Promise<void> {
    if (this.running) {
      this.rerun = true
      return
    }
    this.running = true
    try {
      await this.pass()
    } catch {
      // IO errors wait for the next pass.
    } finally {
      this.running = false
    }
    if (this.rerun) {
      this.rerun = false
      this.poke()
    }
  }

  private async pass(): Promise<void> {
    const [statuses, seen, live, windows] = await Promise.all([
      this.store.readStatuses(),
      this.store.readSeen(),
      this.store.readLiveSessions(),
      this.store.knownWindows(this.pid, this.folders()),
    ])
    const owned: SessionStatus[] = []
    for (const status of statuses) {
      if (live !== null && !live.has(status.sessionId)) {
        continue
      }
      const dir = await this.store.resolveCwd(sessionDir(status, live))
      if (ownerOf(dir, windows) === this.pid) {
        owned.push(status)
      }
    }
    if (this.focused()) {
      for (const status of owned) {
        if (!isUnread(status, seen.get(status.sessionId))) {
          continue
        }
        const now = Date.now()
        try {
          await this.store.writeSeen(status.sessionId, now)
          seen.set(status.sessionId, now)
        } catch {
          // Retried on the next pass while the session stays unread.
        }
      }
    }
    if (this.disposed) {
      return
    }
    const glance = windowGlance(owned, seen)
    const last = this.lastGlance
    if (
      last === null ||
      last.unread !== glance.unread ||
      last.working !== glance.working
    ) {
      this.lastGlance = glance
      this.onGlance(glance)
    }
  }
}
