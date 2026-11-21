import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import type { Server } from 'node:http'
import { createServer, request } from 'node:http'
import type { AddressInfo } from 'node:net'
import {
  connect,
  createServer as createRawServer,
  Server as RawServer,
} from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, expect, it } from 'vitest'

import type { WindowFacts } from './aydyClient'
import type { RevealOutcome, RevealRequest } from './revealSession'
import {
  AydyClient,
  aydyPortFilePath,
  COMMAND_PATH,
  readPublishedPort,
  versionOf,
} from './aydyClient'

const PID = 65_921

interface Reported {
  path: string
  body: Record<string, unknown>
}

/** What a stand-in AYDY answers a report with. */
interface Answer {
  status?: number
  body?: string
}

/**
 * A stand-in for AYDY's loopback listener, recording what this window reported
 * and answering the way the real endpoints do.
 */
class FakeAydy {
  readonly reported: Reported[] = []
  /** Every answer a window gave to a command it sent back. */
  readonly commanded: string[] = []

  /** Windows it is currently holding, by process id. */
  private readonly windows = new Set<number>()
  private answers: Record<string, Answer> = {}
  /** The cadence it asks windows to report on, in seconds. */
  private cadence = 10
  /**
   * Whether registering also confirms the window's callback port, as AYDY
   * does.
   */
  private confirming = false

  private constructor(
    private readonly server: Server,
    readonly port: number,
  ) {}

  static async listening(): Promise<FakeAydy> {
    const server = createServer()

    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', () => {
        resolve()
      })
    })

    const aydy = new FakeAydy(server, (server.address() as AddressInfo).port)

    server.on('request', (incoming, response) => {
      void collect(incoming).then((raw) => {
        const body = JSON.parse(raw) as Record<string, unknown>

        void aydy.handle(incoming.url ?? '', body).then((answer) => {
          response
            .writeHead(answer.status ?? 200, {
              'content-type': 'application/json',
            })
            .end(answer.body ?? '')
        })
      })
    })

    return aydy
  }

  /**
   * Answers `path` with `answer` rather than the way the real endpoint does.
   */
  answering(path: string, answer: Answer): this {
    this.answers[path] = answer

    return this
  }

  /** Asks reporting windows for a beat every `seconds` rather than every 10. */
  beatingEvery(seconds: number): this {
    this.cadence = seconds

    return this
  }

  /**
   * Confirms each registration over the window's own callback port, the way
   * AYDY does.
   */
  confirmsCallbacks(): this {
    this.confirming = true

    return this
  }

  holds(pid: number): boolean {
    return this.windows.has(pid)
  }

  paths(): string[] {
    return this.reported.map((report) => report.path)
  }

  async close(): Promise<void> {
    this.server.closeAllConnections()
    await new Promise<void>((resolve) => {
      this.server.close(() => {
        resolve()
      })
    })
  }

  private async handle(
    path: string,
    body: Record<string, unknown>,
  ): Promise<Answer> {
    this.reported.push({ path, body })

    const configured = this.answers[path]

    if (configured !== undefined) {
      return configured
    }

    const pid = body['pid']

    if (typeof pid !== 'number') {
      return { status: 400, body: registered(false, this.cadence) }
    }

    if (path === '/extension/register') {
      const port = body['callbackPort']

      if (this.confirming && typeof port === 'number') {
        const answered = await this.command(port, pid)

        if (!answered) {
          return { body: registered(false, this.cadence) }
        }
      }

      this.windows.add(pid)

      return { body: registered(true, this.cadence) }
    }

    if (path === '/extension/unregister') {
      this.windows.delete(pid)

      return { body: registered(false, this.cadence) }
    }

    return { body: registered(this.windows.has(pid), this.cadence) }
  }

  /** Posts a ping to a window's callback port, the way AYDY confirms one. */
  private async command(port: number, pid: number): Promise<boolean> {
    const payload = JSON.stringify({ pid, command: 'ping' })
    const answer = await post(port, COMMAND_PATH, payload)

    this.commanded.push(answer ?? '')

    if (answer === null) {
      return false
    }

    const reply = JSON.parse(answer) as { pid?: unknown; ok?: unknown }

    return reply.pid === pid && reply.ok === true
  }
}

const registered = (held: boolean, seconds = 10): string =>
  JSON.stringify({ registered: held, heartbeatSeconds: seconds })

const collect = async (incoming: NodeJS.ReadableStream): Promise<string> => {
  let received = ''

  for await (const chunk of incoming) {
    received += String(chunk)
  }

  return received
}

const post = async (
  port: number,
  path: string,
  payload: string,
): Promise<string | null> =>
  new Promise<string | null>((resolve) => {
    const outgoing = request(
      {
        host: '127.0.0.1',
        port,
        path,
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'content-length': Buffer.byteLength(payload),
        },
      },
      (incoming) => {
        void collect(incoming).then((received) => {
          resolve(incoming.statusCode === 200 ? received : null)
        })
      },
    )

    outgoing.on('error', () => {
      resolve(null)
    })
    outgoing.end(payload)
  })

const opened: {
  clients: AydyClient[]
  aydys: FakeAydy[]
  homes: string[]
  raw: RawServer[]
} = {
  clients: [],
  aydys: [],
  homes: [],
  raw: [],
}

afterEach(async () => {
  await Promise.all(opened.clients.map((client) => client.close()))
  await Promise.all(opened.aydys.map((aydy) => aydy.close()))
  await Promise.all(
    opened.raw.map(
      (server) =>
        new Promise<void>((resolve) => {
          server.close(() => {
            resolve()
          })
        }),
    ),
  )
  await Promise.all(
    opened.homes.map((home) => rm(home, { force: true, recursive: true })),
  )
  opened.clients = []
  opened.aydys = []
  opened.homes = []
  opened.raw = []
})

const aydy = async (): Promise<FakeAydy> => {
  const listening = await FakeAydy.listening()
  opened.aydys.push(listening)

  return listening
}

/** A scratch port file, published or not. */
const portFile = async (port: number | null): Promise<string> => {
  const home = await mkdtemp(join(tmpdir(), 'aydy-extension-'))
  opened.homes.push(home)
  const path = join(home, 'listener.json')

  if (port !== null) {
    await writeFile(path, JSON.stringify({ port }))
  }

  return path
}

const publish = async (path: string, port: number): Promise<void> => {
  await writeFile(path, JSON.stringify({ port }))
}

const facts = (): WindowFacts => ({
  pid: PID,
  workspaceFolders: ['/Users/eric/Code/aydy'],
  claudeCodeVersion: '2.1.238',
})

/** A client already listening for commands, reading `path` for AYDY's port. */
const client = async (
  path: string,
  told: WindowFacts = facts(),
  onRegistered?: () => void,
  reveal?: (request: RevealRequest) => Promise<RevealOutcome>,
): Promise<AydyClient> => {
  const started = new AydyClient({
    facts: told,
    portFilePath: path,
    onRegistered,
    reveal,
  })
  opened.clients.push(started)
  await started.listen()

  return started
}

/** What a window was asked to reveal, and what it answered. */
interface Revealing {
  asked: RevealRequest[]
  reveal: (request: RevealRequest) => Promise<RevealOutcome>
}

const revealing = (outcome: RevealOutcome): Revealing => {
  const asked: RevealRequest[] = []

  return {
    asked,
    reveal: (request) => {
      asked.push(request)

      return Promise.resolve(outcome)
    },
  }
}

/**
 * The port of something that answers with headers and then stops talking, the
 * way a socket that closes after the status line leaves a response: a body that
 * never finishes arriving.
 */
const halfAnswering = async (): Promise<number> => {
  const server = createRawServer((socket) => {
    socket.on('data', () => {
      socket.write(
        'HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 40\r\n\r\n{"registered"',
      )
      socket.end()
    })
  })

  opened.raw.push(server)
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve()
    })
  })

  return (server.address() as AddressInfo).port
}

/**
 * Runs `work` with every attempt to bind a port failing, the way a machine out
 * of file descriptors fails one.
 */
const withNoPortsToBind = async (work: () => Promise<void>): Promise<void> => {
  const bind = RawServer.prototype.listen

  RawServer.prototype.listen = function refusing(this: RawServer): RawServer {
    setImmediate(() => {
      this.emit('error', new Error('EMFILE: too many open files'))
    })

    return this
  }

  try {
    await work()
  } finally {
    RawServer.prototype.listen = bind
  }
}

/** Real time for the scheduled reports to happen in. */
const waited = async (ms: number): Promise<void> =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })

it('finds AYDY where it publishes its port, with nothing configured', () => {
  const path = aydyPortFilePath()

  expect(path.endsWith('/app.aydy/listener.json')).toBe(true)
  expect(path.startsWith('/')).toBe(true)
})

it('reads the port AYDY published', async () => {
  expect(await readPublishedPort(await portFile(51_234))).toBe(51_234)
})

it('reads no port when AYDY has published none', async () => {
  expect(await readPublishedPort(await portFile(null))).toBeNull()
})

/**
 * The file is written by another program, and an unrecognisable one has to read
 * as AYDY not running rather than stopping this window from ever registering.
 */
it('reads no port from a file that does not name one', async () => {
  const path = await portFile(null)

  await writeFile(path, 'not json at all')
  expect(await readPublishedPort(path)).toBeNull()

  await writeFile(path, JSON.stringify({ port: 'fifty thousand' }))
  expect(await readPublishedPort(path)).toBeNull()

  await writeFile(path, JSON.stringify({ port: 0 }))
  expect(await readPublishedPort(path)).toBeNull()
})

it('registers this window with the AYDY the port file names', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))

  await window.report()

  expect(window.connected).toBe(true)
  expect(running.reported).toStrictEqual([
    {
      path: '/extension/register',
      body: {
        pid: PID,
        callbackPort: window.port,
        workspaceFolders: ['/Users/eric/Code/aydy'],
        claudeCodeVersion: '2.1.238',
      },
    },
  ])
})

/**
 * The round trip both later tasks are built on: AYDY confirms the callback port
 * before accepting a registration, and the answer names this window so a port
 * some other process has taken cannot pass for it.
 */
it('answers the command AYDY sends back to it, naming itself', async () => {
  const running = (await aydy()).confirmsCallbacks()
  const window = await client(await portFile(running.port))

  await window.report()

  expect(window.connected).toBe(true)
  expect(running.commanded).toStrictEqual([
    JSON.stringify({ pid: PID, ok: true }),
  ])
})

it('refuses a command it does not know, rather than claiming it ran', async () => {
  const window = await client(await portFile(null))

  const answer = await post(
    window.port ?? 0,
    COMMAND_PATH,
    JSON.stringify({ pid: PID, command: 'revealSomethingFromTheFuture' }),
  )

  expect(answer).toBe(JSON.stringify({ pid: PID, ok: false }))
})

/**
 * The command the whole jump exists for. What was asked is handed to the
 * guard as it arrived, because the tab label is what that guard checks this
 * window's own tabs against.
 */
it('reveals the session AYDY asks this window for', async () => {
  const asked = revealing('revealed')
  const window = await client(
    await portFile(null),
    facts(),
    undefined,
    asked.reveal,
  )

  const answer = await post(
    window.port ?? 0,
    COMMAND_PATH,
    JSON.stringify({
      pid: PID,
      command: 'reveal',
      sessionId: 's1',
      tabLabel: 'Models section configura…',
    }),
  )

  expect(asked.asked).toStrictEqual([
    { sessionId: 's1', tabLabel: 'Models section configura…' },
  ])
  expect(answer).toBe(
    JSON.stringify({ pid: PID, ok: true, reveal: 'revealed' }),
  )
})

/**
 * A window that closes frees the port it named, and the next window to open
 * can be given it. A reveal that lands here for another process is a reveal
 * for a session this window does not have — and calling Claude Code with one
 * of those opens a second live copy of it.
 */
it('refuses a reveal addressed to a window that is not this one', async () => {
  const asked = revealing('revealed')
  const window = await client(
    await portFile(null),
    facts(),
    undefined,
    asked.reveal,
  )

  const answer = await post(
    window.port ?? 0,
    COMMAND_PATH,
    JSON.stringify({
      pid: PID + 1,
      command: 'reveal',
      sessionId: 's1',
      tabLabel: 'Models section configura…',
    }),
  )

  expect(asked.asked).toStrictEqual([])
  expect(answer).toBe(JSON.stringify({ pid: PID, ok: false }))
})

/** The guard's own answer, carried back rather than flattened into a no. */
it('answers a refused reveal with the reason the window gave', async () => {
  const asked = revealing('notPresent')
  const window = await client(
    await portFile(null),
    facts(),
    undefined,
    asked.reveal,
  )

  const answer = await post(
    window.port ?? 0,
    COMMAND_PATH,
    JSON.stringify({
      pid: PID,
      command: 'reveal',
      sessionId: 's1',
      tabLabel: 'Models section configura…',
    }),
  )

  expect(answer).toBe(
    JSON.stringify({ pid: PID, ok: false, reveal: 'notPresent' }),
  )
})

it('refuses a reveal that names no session or no tab', async () => {
  const asked = revealing('revealed')
  const window = await client(
    await portFile(null),
    facts(),
    undefined,
    asked.reveal,
  )

  const answer = await post(
    window.port ?? 0,
    COMMAND_PATH,
    JSON.stringify({ pid: PID, command: 'reveal' }),
  )

  expect(asked.asked).toStrictEqual([])
  expect(answer).toBe(JSON.stringify({ pid: PID, ok: false }))
})

/**
 * AYDY's port is not fixed and it moves when the old one is taken. Nothing
 * tells this window; re-reading the file every time is what follows it.
 */
it('registers with the new port when AYDY moves', async () => {
  const first = await aydy()
  const second = await aydy()
  const path = await portFile(first.port)
  const window = await client(path)
  await window.report()

  await publish(path, second.port)
  await window.report()

  expect(second.paths()).toStrictEqual(['/extension/register'])
  expect(second.holds(PID)).toBe(true)
  expect(window.connected).toBe(true)
})

/**
 * The ordinary state of a machine with VS Code and no AYDY. Reporting has to be
 * a no-op there rather than an error, a prompt, or anything at all.
 */
it('stays quiet when AYDY is not running', async () => {
  const window = await client(await portFile(null))

  await expect(window.report()).resolves.toBeUndefined()

  expect(window.connected).toBe(false)
})

/** AYDY quit, or crashed, and its published port is now nobody's. */
it('stays quiet when the published port answers nothing', async () => {
  const running = await aydy()
  const path = await portFile(running.port)
  await running.close()
  const window = await client(path)

  await expect(window.report()).resolves.toBeUndefined()

  expect(window.connected).toBe(false)
})

it('registers once AYDY is started after this window was', async () => {
  const path = await portFile(null)
  const window = await client(path)
  await window.report()
  expect(window.connected).toBe(false)

  const running = await aydy()
  await publish(path, running.port)
  await window.report()

  expect(window.connected).toBe(true)
  expect(running.holds(PID)).toBe(true)
})

it('says it is still here rather than registering again', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))
  await window.report()

  await window.report()
  await window.report()

  expect(running.paths()).toStrictEqual([
    '/extension/register',
    '/extension/heartbeat',
    '/extension/heartbeat',
  ])
})

/**
 * AYDY was restarted while this window stayed open, so it is holding nothing.
 * Its refusal is the only thing that says so.
 */
it('registers again when AYDY says it does not know this window', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))
  await window.report()

  running.answering('/extension/heartbeat', { body: registered(false) })
  await window.report()

  expect(running.paths()).toStrictEqual([
    '/extension/register',
    '/extension/heartbeat',
    '/extension/register',
  ])
  expect(window.connected).toBe(true)
})

/**
 * A version of AYDY that predates these endpoints answers 200 with an empty
 * body, which is what its listener answers anything it does not route. Reading
 * that as a registration would have this window believe it is connected to
 * something that has never heard of it.
 */
it('is not registered by an answer that says nothing', async () => {
  const running = (await aydy()).answering('/extension/register', { body: '' })
  const window = await client(await portFile(running.port))

  await window.report()

  expect(window.connected).toBe(false)
})

it('tells AYDY the window is closing rather than leaving it to time out', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))
  await window.report()

  await window.close()

  expect(running.paths()).toStrictEqual([
    '/extension/register',
    '/extension/unregister',
  ])
  expect(running.holds(PID)).toBe(false)
})

it('reports the Claude Code version this window is running', () => {
  expect(versionOf({ version: '2.1.238' })).toBe('2.1.238')
})

/**
 * The Claude Code extension is not installed, or its manifest changed shape.
 * Neither stops this window registering: what registration is for is naming
 * which sessions belong here, which does not depend on it.
 */
it('reports no Claude Code version rather than refusing to register', async () => {
  expect(versionOf(undefined)).toBeNull()
  expect(versionOf({})).toBeNull()
  expect(versionOf({ version: 42 })).toBeNull()

  const running = await aydy()
  const path = await portFile(running.port)
  const window = new AydyClient({
    facts: { ...facts(), claudeCodeVersion: null },
    portFilePath: path,
  })
  opened.clients.push(window)
  await window.listen()

  await window.report()

  expect(window.connected).toBe(true)
  expect(running.reported[0]?.body['claudeCodeVersion']).toBeNull()
})

/**
 * "Add Folder to Workspace" does not restart the extension host, so the folders
 * named at activation are only right until the user changes them — and AYDY
 * decides which window a repository is already open in from exactly that list.
 * A heartbeat carries nothing but a process id, so saying so means registering
 * again, which AYDY takes as a replacement rather than a second window.
 */
it('registers again when a folder is added to this window', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))
  await window.report()

  await window.update({
    ...facts(),
    workspaceFolders: ['/Users/eric/Code/aydy', '/Users/eric/Code/okven'],
  })
  await window.report()

  expect(running.paths()).toStrictEqual([
    '/extension/register',
    '/extension/register',
    '/extension/heartbeat',
  ])
  expect(running.reported[1]?.body['workspaceFolders']).toStrictEqual([
    '/Users/eric/Code/aydy',
    '/Users/eric/Code/okven',
  ])
  expect(window.connected).toBe(true)
})

/**
 * The mirror case, and the worse one: a folder AYDY still believes is open here
 * is a folder it will send a session to, and the window it reaches opens a
 * second copy of something it was never holding.
 */
it('registers again when a folder is removed from this window', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port), {
    ...facts(),
    workspaceFolders: ['/Users/eric/Code/aydy', '/Users/eric/Code/okven'],
  })
  await window.report()

  await window.update({
    ...facts(),
    workspaceFolders: ['/Users/eric/Code/aydy'],
  })

  expect(running.paths()).toStrictEqual([
    '/extension/register',
    '/extension/register',
  ])
  expect(running.reported[1]?.body['workspaceFolders']).toStrictEqual([
    '/Users/eric/Code/aydy',
  ])
  expect(window.connected).toBe(true)
})

/**
 * A body that stops arriving after the headers raises nothing on the request:
 * Node destroys the response instead. A report left waiting on it never ends,
 * and a report that never ends is a window that is never heard from again.
 */
it('gives up on an answer whose body never finishes arriving', async () => {
  const window = await client(await portFile(await halfAnswering()))

  await expect(window.report()).resolves.toBeUndefined()

  expect(window.connected).toBe(false)
})

/**
 * The heartbeat is the only thing in a window that ever runs again, so a report
 * that ended badly has to leave the next one armed. Without that, AYDY drops
 * the window when its registration times out and nothing registers it again
 * until VS Code is reloaded.
 */
it('keeps reporting after a report whose answer broke off', async () => {
  const running = (await aydy()).beatingEvery(0.02)
  const broken = await halfAnswering()
  const path = await portFile(running.port)
  const window = await client(path)

  await window.start()
  expect(window.connected).toBe(true)

  await publish(path, broken)
  await waited(150)
  const before = running.paths().length

  await publish(path, running.port)
  await waited(150)

  expect(running.paths().length).toBeGreaterThan(before)
  expect(running.holds(PID)).toBe(true)
})

/**
 * A published port no socket can have is a file that was corrupted or caught
 * half-written. Reporting on it throws from a place nothing catches, and arming
 * the next report is the one thing that cannot be taken down with it.
 */
it('keeps reporting after a report that threw', async () => {
  const running = (await aydy()).beatingEvery(0.02)
  const path = await portFile(running.port)
  const window = await client(path)

  await window.start()
  expect(window.connected).toBe(true)

  await publish(path, 99_999_999)
  await waited(150)
  const before = running.paths().length

  await publish(path, running.port)
  await waited(150)

  expect(running.paths().length).toBeGreaterThan(before)
  expect(running.holds(PID)).toBe(true)
})

/**
 * A bind fails for reasons that pass — descriptors exhausted while a machine
 * woke everything up at once. Giving up on it left this window with no port to
 * be called back on and nothing that would ever try for one again, so AYDY
 * never saw the window at all and nothing said why.
 */
it('binds a command port on a later report when the first bind failed', async () => {
  const running = await aydy()
  const path = await portFile(running.port)
  const window = new AydyClient({ facts: facts(), portFilePath: path })
  opened.clients.push(window)

  await withNoPortsToBind(async () => {
    await window.listen()
    await window.report()
  })

  expect(window.port).toBeNull()
  expect(window.connected).toBe(false)
  expect(running.reported).toStrictEqual([])

  await window.report()

  expect(window.port).not.toBeNull()
  expect(window.connected).toBe(true)
  expect(running.holds(PID)).toBe(true)
})

/**
 * AYDY gave up on a command before it finished sending it. There is nobody left
 * to answer, and a window that let that surface would write a line into the
 * extension host log about something it did nothing to cause.
 */
it('survives a command whose request is abandoned half-sent', async () => {
  const window = await client(await portFile(null))
  const complaints: unknown[] = []
  const complained = (reason: unknown): void => {
    complaints.push(reason)
  }

  process.on('unhandledRejection', complained)

  try {
    const socket = connect(window.port ?? 0, '127.0.0.1')

    await new Promise<void>((resolve) => {
      socket.on('connect', () => {
        resolve()
      })
    })
    socket.write(
      'POST /command HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-type: application/json\r\ncontent-length: 40\r\n\r\n{"pid"',
    )
    await waited(30)
    socket.destroy()
    await waited(30)
  } finally {
    process.off('unhandledRejection', complained)
  }

  expect(complaints).toStrictEqual([])
  expect(
    await post(
      window.port ?? 0,
      COMMAND_PATH,
      JSON.stringify({ pid: PID, command: 'ping' }),
    ),
  ).toBe(JSON.stringify({ pid: PID, ok: true }))
})

it('reports focus and the active tab to AYDY as two separate fields', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))
  await window.report()

  expect(await window.reportFocus(false, 'Fix the auth flow')).toBe(true)

  expect(running.reported.at(-1)).toStrictEqual({
    path: '/extension/focus',
    body: { pid: PID, focused: false, activeTabLabel: 'Fix the auth flow' },
  })
})

/**
 * No Claude tab in front is a different fact from a tab whose session cannot be
 * told, so the field is sent as null rather than left out.
 */
it('reports having no Claude tab active as a null label', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))
  await window.report()

  await window.reportFocus(true, null)

  expect(running.reported.at(-1)).toStrictEqual({
    path: '/extension/focus',
    body: { pid: PID, focused: true, activeTabLabel: null },
  })
})

/**
 * AYDY was restarted while this window stayed open. It only listens to windows
 * it holds, so a focus report from an unheld one has to register first or it
 * reports into nothing for as long as a heartbeat.
 */
it('registers before reporting focus when AYDY does not hold this window', async () => {
  const running = await aydy()
  const window = await client(await portFile(running.port))

  expect(await window.reportFocus(true, 'Fix the auth flow')).toBe(true)

  expect(running.paths()).toStrictEqual([
    '/extension/register',
    '/extension/focus',
  ])
})

/**
 * AYDY was restarted while this window stayed open, and nothing about the
 * window changed while it was gone. Registering again is the only moment
 * anything here can tell that what AYDY held went with it, so it is where the
 * window is asked to say the rest again.
 */
it('asks for what it reports only on change to be said again when it registers', async () => {
  const running = await aydy()
  let asked = 0
  const window = await client(await portFile(running.port), facts(), () => {
    asked += 1
  })
  await window.report()
  expect(asked).toBe(1)

  running.answering('/extension/heartbeat', { body: registered(false) })
  await window.report()

  expect(asked).toBe(2)
})

it('asks for nothing to be said again while a heartbeat is enough', async () => {
  const running = await aydy()
  let asked = 0
  const window = await client(await portFile(running.port), facts(), () => {
    asked += 1
  })
  await window.report()

  await window.report()

  expect(asked).toBe(1)
})

it('asks for nothing to be said again when AYDY took no registration', async () => {
  const running = (await aydy()).answering('/extension/register', {
    body: registered(false),
  })
  let asked = 0
  const window = await client(await portFile(running.port), facts(), () => {
    asked += 1
  })

  await window.report()

  expect(asked).toBe(0)
})

it('answers that a focus report was not taken when AYDY is not running', async () => {
  const window = await client(await portFile(null))

  expect(await window.reportFocus(true, 'Fix the auth flow')).toBe(false)
})

it('answers that a focus report was not taken when AYDY refused it', async () => {
  const running = (await aydy()).answering('/extension/focus', {
    body: registered(false),
  })
  const window = await client(await portFile(running.port))
  await window.report()

  expect(await window.reportFocus(true, 'Fix the auth flow')).toBe(false)
})

/**
 * An AYDY that predates this endpoint answers 200 with an empty body, the same
 * as it answers anything else it does not route. Only a body makes an answer.
 */
it('answers that a focus report was not taken by an AYDY that cannot receive one', async () => {
  const running = (await aydy()).answering('/extension/focus', {})
  const window = await client(await portFile(running.port))
  await window.report()

  expect(await window.reportFocus(true, 'Fix the auth flow')).toBe(false)
})
