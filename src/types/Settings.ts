/**
 * The settings as the pane renders them.
 *
 * Every value here is what the store settled on rather than what the pane asked
 * for.
 */
export interface Settings {
  finishedSound: boolean
  needsInputSound: boolean
  launchAtLogin: boolean
}
