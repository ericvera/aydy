/**
 * One entry an install will remove from the user's settings, named by the
 * command it runs.
 */
export interface RemovedHook {
  event: string
  command: string | null
}
