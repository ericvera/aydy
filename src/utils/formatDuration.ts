const SECOND_MS = 1_000
const MINUTE_MS = 60 * SECOND_MS
const HOUR_MS = 60 * MINUTE_MS

/**
 * A duration as the popover writes it: `1h 12m`, `6m`, `45s`.
 *
 * Rounded down, so a duration that is still running never reads as more than
 * it is.
 */
export const formatDuration = (durationMs: number): string => {
  const total = Math.max(0, Math.floor(durationMs))

  if (total >= HOUR_MS) {
    const minutes = Math.floor((total % HOUR_MS) / MINUTE_MS)

    return `${Math.floor(total / HOUR_MS)}h ${minutes}m`
  }

  if (total >= MINUTE_MS) {
    return `${Math.floor(total / MINUTE_MS)}m`
  }

  return `${Math.floor(total / SECOND_MS)}s`
}
