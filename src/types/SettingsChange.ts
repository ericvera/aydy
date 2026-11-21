/**
 * A change to some of the settings.
 *
 * Every field is optional so a control that moved sends only itself: sending
 * back the values it did not touch is how a stale copy of the settings
 * overwrites a change made somewhere else.
 */
export interface SettingsChange {
  finishedSound?: boolean
  needsInputSound?: boolean
  launchAtLogin?: boolean
}
