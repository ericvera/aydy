/**
 * Whether the extension's hook script and its hooks are in place: the script
 * absent is `notInstalled`, the script present with its hooks gone from the
 * settings file is `hooksMissing`.
 */
export type TrackingState = 'installed' | 'notInstalled' | 'hooksMissing'

/** Whether session tracking is installed, as the popover reads it. */
export interface TrackingStatus {
  state: TrackingState
}
