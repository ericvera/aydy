import { homedir } from 'node:os'
import { join } from 'node:path'

export interface PathOptions {
  home?: string
  env?: NodeJS.ProcessEnv
}

/** Identifies AYDY's hook command in Claude's settings, whatever the home. */
export const HOOK_SCRIPT_MARKER = 'app.aydy/bin/aydy-hook.sh'

/** The hook script's location relative to the installed extension's root. */
export const HOOK_SCRIPT_SOURCE = 'hook/aydy-hook.sh'

export const aydyDataDir = ({ home = homedir() }: PathOptions = {}): string =>
  join(home, 'Library', 'Application Support', 'app.aydy')

export const statusDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'status')

export const seenDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'seen')

export const progressDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'progress')

export const jumpDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'jump')

export const windowsDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'windows')

export const workspacesDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'workspaces')

export const noticesDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'notices')

export const binDir = (options: PathOptions = {}): string =>
  join(aydyDataDir(options), 'bin')

export const hookScriptPath = (options: PathOptions = {}): string =>
  join(binDir(options), 'aydy-hook.sh')

export const claudeConfigDir = ({
  home = homedir(),
  env = process.env,
}: PathOptions = {}): string => {
  const override = env['CLAUDE_CONFIG_DIR']
  return override !== undefined && override !== ''
    ? override
    : join(home, '.claude')
}

export const claudeSettingsPath = (options: PathOptions = {}): string =>
  join(claudeConfigDir(options), 'settings.json')

export const claudeSessionsDir = (options: PathOptions = {}): string =>
  join(claudeConfigDir(options), 'sessions')
