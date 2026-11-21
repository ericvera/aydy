/**
 * Whether AYDY's hooks are registered in the user's Claude Code settings.
 *
 * Read from the file itself, not from what the app remembers doing: the file
 * belongs to the user, and hooks taken out of it are what put the first-run
 * screen back in front of them.
 */
export interface HookStatus {
  installed: boolean
  hookCount: number
  settingsPath: string
  /**
   * Whether the events those hooks send are reaching the app — a second fact,
   * not a restatement of `installed`. Hooks posting to a port nothing is
   * listening on are installed and silent, and the settings pane keeps the two
   * apart because that state is the one worth acting on.
   */
  receivingEvents: boolean
}
