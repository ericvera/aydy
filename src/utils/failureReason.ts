const UNEXPLAINED = 'AYDY could not tell what went wrong.'

/**
 * The reason a rejected command carried.
 *
 * Every command the popover invokes rejects with a reason, so anything else is
 * a bug rather than a state the user can act on and must not be rendered as
 * one: an exception object stringified into a banner is not a sentence anyone
 * can do anything with.
 */
export const failureReason = (error: unknown): string => {
  if (
    typeof error === 'object' &&
    error !== null &&
    'reason' in error &&
    typeof error.reason === 'string'
  ) {
    return error.reason
  }

  return UNEXPLAINED
}
