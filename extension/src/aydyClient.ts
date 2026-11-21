import { readFile } from 'node:fs/promises'
import type { IncomingMessage, Server, ServerResponse } from 'node:http'
import { createServer, request } from 'node:http'
import { homedir } from 'node:os'
import { join } from 'node:path'

import type { RevealOutcome, RevealRequest } from './revealSession'

/**
 * AYDY's bundle identifier, which names the directory it keeps its files in.
 */
const AYDY_IDENTIFIER = 'app.aydy'

/** The file AYDY publishes its current listener port in. */
const PORT_FILE = 'listener.json'

const LOOPBACK = '127.0.0.1'

const REGISTER_PATH = '/extension/register'
const HEARTBEAT_PATH = '/extension/heartbeat'
const UNREGISTER_PATH = '/extension/unregister'
const FOCUS_PATH = '/extension/focus'

/** The path this window answers AYDY's commands on. */
export const COMMAND_PATH = '/command'

/**
 * How often this window says it is still open, until AYDY says otherwise.
 *
 * AYDY names its own cadence in every answer, and that is what is used once one
 * has been received; this covers the moments before the first.
 */
const DEFAULT_HEARTBEAT_MS = 10_000

/** How long a report to AYDY waits before being treated as unanswered. */
const REQUEST_TIMEOUT_MS = 2_000

/** What this window tells AYDY about itself. */
export interface WindowFacts {
  /**
   * This extension host's own process id.
   *
   * VS Code has no window identifier of any kind, but every Claude Code session
   * started in a window is a descendant of that window's extension host — so
   * this number is what lets AYDY say which sessions belong here.
   */
  pid: number
  /** The folders open in this window, as absolute paths. */
  workspaceFolders: string[]
  /**
   * The version of the Claude Code extension in this window, or `null` when it
   * is not installed.
   */
  claudeCodeVersion: string | null
}

export interface AydyClientOptions {
  facts: WindowFacts
  /** The file AYDY publishes its port in. */
  portFilePath: string
  /**
   * Called each time AYDY takes a registration of this window.
   *
   * A registration is the whole of what a freshly started AYDY holds about a
   * window, so whatever this window reports only when it changes has to be
   * said again from here: the AYDY that took it last is gone, and nothing
   * else will ask.
   */
  onRegistered?: (() => void) | undefined
  /**
   * Brings a session's own tab to the front of this window.
   *
   * Passed in rather than reached for, because everything it needs lives on
   * the `vscode` API and nothing in this file may import it. A client without
   * one refuses reveals, which is the safe answer: AYDY falls back to raising
   * the window.
   */
  reveal?: ((request: RevealRequest) => Promise<RevealOutcome>) | undefined
}

/** What this window answers a command with. */
interface CommandAnswer {
  ok: boolean
  reveal?: RevealOutcome
}

/** AYDY's answer to a window reporting itself. */
interface RegisteredAnswer {
  registered: boolean
  heartbeatSeconds?: number
}

/**
 * Where AYDY publishes the port it is listening on.
 *
 * Discovered rather than configured: the port is chosen by AYDY and moves when
 * it has to, so a setting holding it would be wrong as often as it was right —
 * and the extension is required to need no configuration at all.
 */
export const aydyPortFilePath = (): string =>
  join(homedir(), 'Library', 'Application Support', AYDY_IDENTIFIER, PORT_FILE)

/**
 * The port AYDY has published, or `null` when it is not running.
 *
 * A missing file is the normal state on a machine where AYDY is not installed,
 * so it is answered rather than reported.
 */
export const readPublishedPort = async (
  path: string,
): Promise<number | null> => {
  try {
    const published: unknown = JSON.parse(await readFile(path, 'utf8'))

    if (typeof published !== 'object' || published === null) {
      return null
    }

    const port = (published as { port?: unknown }).port

    return typeof port === 'number' && Number.isInteger(port) && port > 0
      ? port
      : null
  } catch {
    return null
  }
}

/**
 * The version an extension manifest declares, or `null` when it declares none.
 *
 * `packageJSON` is whatever the extension shipped, so nothing about its shape
 * can be assumed — the Claude Code integration depends on the internals of a
 * bundle that ships roughly weekly, and a manifest that has changed must leave
 * this window registered rather than failing it.
 */
export const versionOf = (packageJson: unknown): string | null => {
  if (typeof packageJson !== 'object' || packageJson === null) {
    return null
  }

  const version = (packageJson as { version?: unknown }).version

  return typeof version === 'string' && version.length > 0 ? version : null
}

/**
 * This window's side of the conversation with AYDY.
 *
 * It reports facts and executes commands, and holds no opinion about sessions:
 * which session belongs to which window, and which tab shows which session, are
 * AYDY's to work out, because AYDY is the only side that can see every window
 * at once.
 *
 * Both directions run over loopback HTTP. This window binds a port of its own
 * and names it when it registers, so AYDY can address this window and no other
 * — a command sent to the wrong window is how a second, live copy of a Claude
 * session gets opened.
 */
export class AydyClient {
  private facts: WindowFacts
  private readonly portFilePath: string
  private readonly onRegistered: (() => void) | undefined
  private readonly revealer:
    ((request: RevealRequest) => Promise<RevealOutcome>) | undefined

  private server: Server | null = null
  private callbackPort: number | null = null
  /** The AYDY port this window is currently registered with. */
  private registeredPort: number | null = null
  private registered = false
  /** Whether what AYDY holds for this window has since changed. */
  private stale = false
  private heartbeatMs = DEFAULT_HEARTBEAT_MS
  private timer: NodeJS.Timeout | null = null
  private running = false

  constructor(options: AydyClientOptions) {
    this.facts = options.facts
    this.portFilePath = options.portFilePath
    this.onRegistered = options.onRegistered
    this.revealer = options.reveal
  }

  /** Whether AYDY currently holds this window's registration. */
  get connected(): boolean {
    return this.registered
  }

  /** The loopback port this window takes commands on. */
  get port(): number | null {
    return this.callbackPort
  }

  /** Binds the port this window takes commands on. */
  async listen(): Promise<void> {
    if (this.server !== null) {
      return
    }

    const server = createServer((incoming, response) => {
      this.answer(incoming, response)
    })

    // The extension host outlives this either way, and an unreferenced handle
    // cannot be the reason a process refuses to exit.
    server.unref()

    await new Promise<void>((resolve) => {
      server.once('error', () => {
        resolve()
      })
      server.listen(0, LOOPBACK, () => {
        resolve()
      })
    })

    const address = server.address()

    if (address === null || typeof address === 'string') {
      server.close()

      return
    }

    this.callbackPort = address.port
    this.server = server
  }

  /**
   * Tells AYDY this window is here, once.
   *
   * The port file is read every time rather than remembered, so a port that
   * moved is followed on the next report and nothing has to be told about the
   * move. AYDY not running at all is the ordinary case and leaves this window
   * quietly unregistered.
   */
  async report(): Promise<void> {
    const port = await readPublishedPort(this.portFilePath)

    if (port === null) {
      this.registered = false
      this.registeredPort = null

      return
    }

    if (this.registered && port === this.registeredPort && !this.stale) {
      if (await this.beat(port)) {
        return
      }
    }

    await this.register(port)
  }

  /**
   * Reports facts about this window that have changed since it last said them.
   *
   * A heartbeat carries nothing but a process id, so the only way to say that
   * a folder was opened or closed is to register again — which AYDY takes as
   * a replacement for what it holds, not as a second window. Folders change
   * without the extension host restarting, and a folder AYDY believes this
   * window still holds is a folder it will send a session to.
   */
  async update(facts: WindowFacts): Promise<void> {
    this.facts = facts
    this.stale = true

    await this.report()
  }

  /**
   * Tells AYDY whether this window has focus and what its active Claude tab is
   * labelled, answering whether AYDY took the report.
   *
   * The two travel as two fields and are never combined here. Which session the
   * label belongs to is AYDY's to decide, because this window holds neither the
   * session titles nor the knowledge of which sessions run in it.
   *
   * AYDY only listens to windows it holds, so a report from an unregistered one
   * registers first. That is the state after AYDY restarted, and it is exactly
   * when the user is at the keyboard and the report matters most. Registering
   * is also what asks for the state to be said again, so an AYDY that came
   * back while nothing moved is told where the user is on the next beat rather
   * than on the next thing they do.
   */
  async reportFocus(
    focused: boolean,
    activeTabLabel: string | null,
  ): Promise<boolean> {
    if (!this.registered) {
      await this.report()
    }

    const port = this.registeredPort

    if (port === null) {
      return false
    }

    const answer = await this.post(port, FOCUS_PATH, {
      pid: this.facts.pid,
      focused,
      activeTabLabel,
    })

    return answer?.registered === true
  }

  /** Starts reporting, and keeps reporting until [`close`] is called. */
  async start(): Promise<void> {
    this.running = true

    await this.listen()
    await this.report()

    this.schedule()
  }

  /**
   * Stops reporting and tells AYDY this window is going.
   *
   * Saying so is not required — a window that stops answering is dropped on its
   * own — but it is the difference between the settings pane being right
   * immediately and being right in half a minute.
   */
  async close(): Promise<void> {
    this.running = false

    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }

    const port = this.registeredPort

    if (port !== null) {
      await this.post(port, UNREGISTER_PATH, { pid: this.facts.pid })
    }

    this.registered = false
    this.registeredPort = null

    const server = this.server
    this.server = null
    this.callbackPort = null

    if (server !== null) {
      server.closeAllConnections()
      await new Promise<void>((resolve) => {
        server.close(() => {
          resolve()
        })
      })
    }
  }

  private schedule(): void {
    this.timer = setTimeout(() => {
      void this.reportAndSchedule()
    }, this.heartbeatMs)

    this.timer.unref()
  }

  /**
   * One scheduled report, and the arming of the one after it.
   *
   * This timer is the only thing in this window that ever runs again, so what
   * arms it cannot be reached only on the paths where a report went well: a
   * report that ended any other way would take every later one with it, and
   * the window would go quiet until VS Code was reloaded.
   */
  private async reportAndSchedule(): Promise<void> {
    try {
      await this.report()
    } catch {
      // Reporting again on the next beat is the whole of the recovery.
    }

    if (this.running) {
      this.schedule()
    }
  }

  private async register(port: number): Promise<void> {
    if (this.callbackPort === null) {
      // A bind fails for reasons that pass — descriptors exhausted while the
      // machine woke everything up at once — and a window that gave up on the
      // first one would stay invisible to AYDY for as long as it was open.
      await this.listen()
    }

    const callbackPort = this.callbackPort

    if (callbackPort === null) {
      this.registered = false
      this.registeredPort = null

      return
    }

    const answer = await this.post(port, REGISTER_PATH, {
      pid: this.facts.pid,
      callbackPort,
      workspaceFolders: this.facts.workspaceFolders,
      claudeCodeVersion: this.facts.claudeCodeVersion,
    })

    this.accept(answer)
    this.registeredPort = this.registered ? port : null
    // What AYDY holds is only current if AYDY took it.
    this.stale = !this.registered

    if (this.registered) {
      // Everything else this window says, it says only when it changes, so a
      // registration AYDY has just taken is the one moment the rest can be
      // asked for again. A refused one holds nothing worth telling.
      this.onRegistered?.()
    }
  }

  private async beat(port: number): Promise<boolean> {
    this.accept(await this.post(port, HEARTBEAT_PATH, { pid: this.facts.pid }))

    return this.registered
  }

  /**
   * Takes AYDY's answer, including the cadence it wants to be told on.
   *
   * Following AYDY's number rather than a constant compiled in here is what
   * keeps the two in step: this extension is installed and updated separately
   * from the app, so the version answering is rarely the version that shipped
   * alongside it.
   */
  private accept(answer: RegisteredAnswer | null): void {
    this.registered = answer?.registered === true

    const seconds = answer?.heartbeatSeconds

    if (typeof seconds === 'number' && seconds > 0) {
      this.heartbeatMs = seconds * 1000
    }
  }

  /**
   * Posts one report to AYDY, answering `null` when it did not arrive.
   *
   * A body is what makes an answer an answer: AYDY's listener replies 200 with
   * an empty body to anything it does not route, so a version that predates
   * these endpoints would otherwise look like a successful registration.
   */
  private async post(
    port: number,
    path: string,
    body: unknown,
  ): Promise<RegisteredAnswer | null> {
    const payload = JSON.stringify(body)

    const answer = await new Promise<string | null>((resolve) => {
      const outgoing = request(
        {
          host: LOOPBACK,
          port,
          path,
          method: 'POST',
          timeout: REQUEST_TIMEOUT_MS,
          headers: {
            'content-type': 'application/json',
            'content-length': Buffer.byteLength(payload),
          },
        },
        (incoming) => {
          void collect(incoming)
            .then((received) => {
              resolve(incoming.statusCode === 200 ? received : null)
            })
            // A body that stops arriving — the socket closed after the
            // headers, or the timeout fired mid-read — raises nothing on the
            // request, so this is the only place left that can end the wait.
            .catch(() => {
              resolve(null)
            })
        },
      )

      outgoing.on('timeout', () => {
        outgoing.destroy()
      })
      // AYDY not running is the ordinary state of the world, so a connection
      // that is refused is an answer of no rather than something to report.
      outgoing.on('error', () => {
        resolve(null)
      })
      outgoing.end(payload)
    })

    if (answer === null) {
      return null
    }

    try {
      const parsed: unknown = JSON.parse(answer)

      if (
        typeof parsed === 'object' &&
        parsed !== null &&
        typeof (parsed as { registered?: unknown }).registered === 'boolean'
      ) {
        return parsed as RegisteredAnswer
      }
    } catch {
      return null
    }

    return null
  }

  /**
   * Answers one command from AYDY.
   *
   * Every answer names this window, because a port is only an address: a window
   * that closed leaves its port to be taken by something else, and AYDY has no
   * other way to tell that it is talking to a stranger.
   */
  private answer(incoming: IncomingMessage, response: ServerResponse): void {
    if (incoming.method !== 'POST' || incoming.url !== COMMAND_PATH) {
      response.writeHead(404).end()

      return
    }

    void collect(incoming)
      .then(async (body) => this.run(body))
      .then((answer) => {
        response
          .writeHead(200, { 'content-type': 'application/json' })
          .end(JSON.stringify({ pid: this.facts.pid, ...answer }))
      })
      // AYDY gave up on the command before it finished sending it, so there
      // is nobody left to answer. Letting that surface would put a line in
      // the extension host log about a window that did nothing wrong.
      .catch(() => {
        response.destroy()
      })
  }

  /**
   * Runs one command, answering what became of it.
   *
   * A command this window does not understand is refused rather than treated
   * as done: AYDY is updated separately from this extension, so a newer one
   * asking for something added since is an ordinary state of the world.
   */
  private async run(body: string): Promise<CommandAnswer> {
    let command: unknown

    try {
      command = JSON.parse(body)
    } catch {
      return { ok: false }
    }

    if (typeof command !== 'object' || command === null) {
      return { ok: false }
    }

    const asked = command as {
      command?: unknown
      pid?: unknown
      sessionId?: unknown
      tabLabel?: unknown
    }

    if (asked.command === 'ping') {
      return { ok: true }
    }

    if (asked.command !== 'reveal') {
      return { ok: false }
    }

    // A port is only an address. This window's closing frees the one it named,
    // and the next window to open can be given it — so a reveal that arrives
    // here addressed to some other process is a reveal for a session this
    // window does not have, and calling Claude Code with it is precisely how a
    // second live copy of that session gets started.
    if (asked.pid !== this.facts.pid) {
      return { ok: false }
    }

    if (
      this.revealer === undefined ||
      typeof asked.sessionId !== 'string' ||
      typeof asked.tabLabel !== 'string'
    ) {
      return { ok: false }
    }

    const outcome = await this.revealer({
      sessionId: asked.sessionId,
      tabLabel: asked.tabLabel,
    })

    return { ok: outcome === 'revealed', reveal: outcome }
  }
}

const collect = async (incoming: IncomingMessage): Promise<string> => {
  let received = ''

  for await (const chunk of incoming) {
    received += String(chunk)
  }

  return received
}
