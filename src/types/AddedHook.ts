/**
 * One hook an install will register.
 */
export interface AddedHook {
  event: string
  kind: 'command' | 'http'
  timeoutSeconds: number
}
