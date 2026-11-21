import type { AddedHook } from './AddedHook'
import type { RemovedHook } from './RemovedHook'

/**
 * The exact change an install will make, worked out by the installer without
 * writing anything.
 *
 * `hookCount` is counted from what will actually be registered, so the number
 * the first-run screen states cannot drift from the number of hooks written.
 */
export interface HookPreview {
  hookCount: number
  added: AddedHook[]
  removed: RemovedHook[]
  hooksJson: string
}
