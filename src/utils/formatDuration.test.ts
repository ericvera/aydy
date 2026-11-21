import { expect, it } from 'vitest'

import { formatDuration } from './formatDuration'

it('writes seconds under a minute', () => {
  expect(formatDuration(45_000)).toBe('45s')
})

it('writes nothing at all as zero seconds', () => {
  expect(formatDuration(0)).toBe('0s')
})

it('writes whole minutes under an hour', () => {
  expect(formatDuration(2_400_000)).toBe('40m')
})

it('writes hours and minutes together', () => {
  expect(formatDuration(4_320_000)).toBe('1h 12m')
})

it('keeps the hour when there are no minutes with it', () => {
  expect(formatDuration(3_600_000)).toBe('1h 0m')
})

/**
 * Rounded down, so a duration that is still running never reads as more than
 * it is.
 */
it('rounds down rather than to the nearest', () => {
  expect(formatDuration(119_999)).toBe('1m')
})

it('treats a negative duration as none', () => {
  expect(formatDuration(-1)).toBe('0s')
})
