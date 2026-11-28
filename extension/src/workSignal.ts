import { isWorking } from './sessionState'
import type { SessionStatus, TitleSignal } from './sessionState'

export const STALE_AFTER_MS = 5 * 60 * 1000

const MAX_ETA_SECONDS = 24 * 60 * 60
const MINUTE_MS = 60 * 1000
const HOUR_MS = 60 * MINUTE_MS

export interface Step {
  done: number
  total: number
}

export interface ProgressReport {
  etaMs: number
  step: Step | null
  reportedAt: number
}

export interface SessionActivity {
  status: SessionStatus
  turnStartedAt: number | null
  transcriptWrittenAt: number | null
  progress: ProgressReport | null
}

export interface SessionSignal {
  stale: boolean
  quietMs: number | null
  elapsedMs: number | null
  step: Step | null
  etaRemainingMs: number | null
}

export const nextTurnStart = (
  previous: number | null,
  status: SessionStatus,
): number | null => {
  if (isWorking(status)) {
    return previous ?? status.eventAt
  }
  return status.event === 'Stop' ? null : previous
}

const field = (value: unknown, key: string): unknown =>
  typeof value === 'object' && value !== null
    ? (value as Record<string, unknown>)[key]
    : undefined

const isCount = (value: unknown): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= 0

const parseStep = (value: unknown): Step | null => {
  const done = field(value, 'done')
  const total = field(value, 'total')
  return isCount(done) && isCount(total) && total >= 1 && done <= total
    ? { done, total }
    : null
}

export const parseProgress = (
  text: string,
  mtimeMs: number,
): ProgressReport | null => {
  let payload: unknown
  try {
    payload = JSON.parse(text)
  } catch {
    return null
  }
  const etaSeconds = field(payload, 'etaSeconds')
  if (!isCount(etaSeconds)) {
    return null
  }
  return {
    etaMs: Math.min(etaSeconds, MAX_ETA_SECONDS) * 1000,
    step: parseStep(field(payload, 'step')),
    reportedAt: mtimeMs,
  }
}

export const sessionSignal = (
  { status, turnStartedAt, transcriptWrittenAt, progress }: SessionActivity,
  now: number,
): SessionSignal | null => {
  if (!isWorking(status)) {
    return null
  }
  const accepted =
    progress !== null &&
    (turnStartedAt === null || progress.reportedAt >= turnStartedAt)
      ? progress
      : null
  const quietMs =
    transcriptWrittenAt === null
      ? null
      : Math.max(
          0,
          now -
            Math.max(
              transcriptWrittenAt,
              status.eventAt,
              accepted?.reportedAt ?? -Infinity,
            ),
        )
  return {
    stale: quietMs !== null && quietMs >= STALE_AFTER_MS,
    quietMs,
    elapsedMs: turnStartedAt === null ? null : Math.max(0, now - turnStartedAt),
    step: accepted?.step ?? null,
    etaRemainingMs:
      accepted === null
        ? null
        : accepted.etaMs - Math.max(0, now - accepted.reportedAt),
  }
}

const tier = (signal: SessionSignal): number => {
  if (signal.stale) {
    return 0
  }
  if (signal.etaRemainingMs !== null) {
    return 1
  }
  return signal.elapsedMs === null ? 3 : 2
}

const moreUrgent = (a: SessionSignal, b: SessionSignal): boolean => {
  const tierA = tier(a)
  const tierB = tier(b)
  if (tierA !== tierB) {
    return tierA < tierB
  }
  switch (tierA) {
    case 0:
      return (a.quietMs ?? 0) > (b.quietMs ?? 0)
    case 1:
      return (a.etaRemainingMs ?? 0) < (b.etaRemainingMs ?? 0)
    case 2:
      return (a.elapsedMs ?? 0) > (b.elapsedMs ?? 0)
    default:
      return false
  }
}

export const mostUrgent = (
  signals: readonly SessionSignal[],
): SessionSignal | null =>
  signals.reduce<SessionSignal | null>(
    (best, signal) =>
      best === null || moreUrgent(signal, best) ? signal : best,
    null,
  )

export const formatMinutes = (ms: number): string => {
  if (ms < MINUTE_MS) {
    return '<1m'
  }
  if (ms < HOUR_MS) {
    return `${String(Math.floor(ms / MINUTE_MS))}m`
  }
  const hours = Math.floor(ms / HOUR_MS)
  const minutes = Math.floor((ms % HOUR_MS) / MINUTE_MS)
  return `${String(hours)}h ${String(minutes)}m`
}

const etaText = (remainingMs: number): string =>
  remainingMs < 0
    ? `${formatMinutes(-remainingMs)} over`
    : `~${formatMinutes(remainingMs)}`

export const titleSignal = (signal: SessionSignal): TitleSignal | null => {
  if (signal.stale && signal.quietMs !== null) {
    return { stale: true, text: `quiet ${formatMinutes(signal.quietMs)}` }
  }
  const { step, etaRemainingMs, elapsedMs } = signal
  if (etaRemainingMs !== null) {
    const eta = etaText(etaRemainingMs)
    return {
      stale: false,
      text:
        step === null
          ? eta
          : `${String(step.done)}/${String(step.total)} · ${eta}`,
    }
  }
  if (elapsedMs !== null) {
    return { stale: false, text: formatMinutes(elapsedMs) }
  }
  return null
}
