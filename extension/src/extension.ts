import { realpathSync } from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname } from 'node:path'

import * as vscode from 'vscode'

import { claudeSettingsPath, jumpDir } from './aydyPaths'
import {
  claimFailureNotice,
  failureMessage,
  releaseFailureNotice,
} from './hookNotice'
import { ensureHooks, installHookScript } from './hookInstall'
import { JumpResponder } from './jumpRequests'
import {
  countActive,
  listReasons,
  reopenAsWorkspace,
  unsafeReasons,
} from './autoReopen'
import type { RevealEnvironment } from './revealSession'
import { revealSession, versionOf } from './revealSession'
import { folderLabel } from './repoLabel'
import { tabTitle } from './sessionState'
import type { WindowGlance } from './sessionState'
import {
  hasChildren,
  readLiveSessions,
  readParents,
  sessionIdsUnder,
} from './processTree'
import { readStatuses } from './sessionStore'
import { publishWindow, unpublishWindow } from './windowRegistry'
import { WindowTracker } from './windowTracker'
import {
  isLegacyWorkspace,
  readWorkspaceFolder,
  registerWorkspace,
  removeLegacyWorkspace,
  setWorkspaceTitle,
  siblingWorkspacePath,
  sweepWorkspaces,
  syncFolderSettings,
  workspaceState,
  writeWorkspace,
} from './workspaceFile'

/**
 * The Claude Code extension, whose version this integration is pinned against.
 *
 * It exports no API, so nothing here calls it. Its version is read only so the
 * reveal can refuse to drive a Claude Code version this integration has not
 * been checked against.
 */
const CLAUDE_CODE_EXTENSION = 'Anthropic.claude-code'

/**
 * The webview panel Claude Code opens a session in, which is what a tab showing
 * a session is made of.
 *
 * The editor prefixes the view type an extension chose, so this is tested by
 * containment rather than equality. Getting it wrong leaves every tab looking
 * like no Claude tab at all, so a reveal answers `notPresent` — the safe
 * direction — whereas matching any tab would let a source file named like a
 * session summary be taken for a session's tab.
 */
const CLAUDE_PANEL_VIEW_TYPE = 'claudeVSCodePanel'

/**
 * The command Claude Code contributes to open a session in a tab.
 *
 * Public — it is in the extension's `contributes.commands` — but its arguments
 * are not, and calling it for a session this window does not have open creates
 * a second live copy of that session. `revealSession.ts` is the guard; this is
 * only the call.
 */
const REVEAL_COMMAND = 'claude-vscode.editor.open'

/** Whether `tab` is a Claude Code session rather than an editor. */
const isClaudeTab = (tab: vscode.Tab): boolean =>
  tab.input instanceof vscode.TabInputWebview &&
  tab.input.viewType.includes(CLAUDE_PANEL_VIEW_TYPE)

/** The version of the Claude Code extension installed in this window. */
const claudeCodeVersion = (): string | null =>
  versionOf(vscode.extensions.getExtension(CLAUDE_CODE_EXTENSION)?.packageJSON)

let glance: WindowGlance = { unread: 0, working: false }

let label: string | undefined

const log = (error: unknown): void => {
  console.error('AYDY:', error)
}

let workspaceWrites: Promise<unknown> = Promise.resolve()

const enqueueWorkspaceWrite = (fn: () => Promise<unknown>): void => {
  workspaceWrites = workspaceWrites.then(fn).catch(log)
}

let generated: { path: string; folder: string } | undefined

const writeTitle = (): void => {
  const target = generated
  if (target === undefined) {
    return
  }
  enqueueWorkspaceWrite(() =>
    setWorkspaceTitle(
      target.path,
      tabTitle(glance, label ?? basename(target.folder)),
    ),
  )
}

const refreshLabel = async (folder: string): Promise<string | null> => {
  const next = await folderLabel(folder)
  label = next.label
  writeTitle()
  return next.headPath
}

const watchHead = (
  context: vscode.ExtensionContext,
  folder: string,
  headPath: string,
): void => {
  const refresh = (): void => {
    refreshLabel(folder).catch(log)
  }
  const watcher = vscode.workspace.createFileSystemWatcher(
    new vscode.RelativePattern(vscode.Uri.file(dirname(headPath)), 'HEAD'),
  )
  context.subscriptions.push(
    watcher,
    watcher.onDidCreate(refresh),
    watcher.onDidChange(refresh),
    watcher.onDidDelete(refresh),
  )
}

const syncSettings = (path: string): void => {
  enqueueWorkspaceWrite(() => syncFolderSettings(path))
}

const openWorkspaceFile = (): string | undefined => {
  const file = vscode.workspace.workspaceFile
  return file?.scheme === 'file' ? file.fsPath : undefined
}

const keepGeneratedWorkspace = async (
  context: vscode.ExtensionContext,
  path: string,
  folder: string,
): Promise<void> => {
  generated = { path, folder }
  syncSettings(path)
  refreshLabel(folder)
    .then((headPath) => {
      if (headPath !== null) {
        watchHead(context, folder, headPath)
      }
    })
    .catch(log)
  const watcher = vscode.workspace.createFileSystemWatcher(
    new vscode.RelativePattern(
      vscode.Uri.file(folder),
      '.vscode/settings.json',
    ),
  )
  context.subscriptions.push(
    watcher,
    watcher.onDidCreate(() => {
      syncSettings(path)
    }),
    watcher.onDidChange(() => {
      syncSettings(path)
    }),
    watcher.onDidDelete(() => {
      syncSettings(path)
    }),
  )
}

/** The setting that turns automatic reopening off. */
const AUTO_REOPEN_SETTING = 'autoReopenAsWorkspace'

/**
 * How long a freshly opened window is left to settle before AYDY looks at what
 * is in it: VS Code restores terminals and Claude Code restores its tabs just
 * after startup, and reopening before they show up would end them unseen.
 */
const SETTLE_MS = 1500

/** How long to wait for a shell pid; a restored terminal may never send one. */
const TERMINAL_PID_MS = 1000

const REOPEN_ANYWAY = 'Reopen anyway'
const NOT_NOW = 'Not now'
const OVERWRITE = 'Overwrite'
const SINGLE_FOLDER_ONLY =
  'AYDY: The session count in the tab title only applies to a window opened as a single folder.'

const tildePath = (path: string): string => {
  const home = homedir()
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path
}

const confirmOverwrite = async (path: string): Promise<boolean> =>
  (await vscode.window.showWarningMessage(
    `${tildePath(path)} already exists and was not created by AYDY. Overwrite it?`,
    {
      modal: true,
      detail:
        'AYDY keeps this window’s workspace file beside the folder so it is easy to find. If you keep the existing file, AYDY stores its workspace file in its own data folder instead.',
    },
    OVERWRITE,
  )) === OVERWRITE

const openFolder = (
  path: string,
  options: { forceReuseWindow: boolean },
): Thenable<unknown> =>
  vscode.commands.executeCommand(
    'vscode.openFolder',
    vscode.Uri.file(path),
    options,
  )

const openWorkspace = async (folder: string): Promise<void> => {
  await reopenAsWorkspace(
    folder,
    tabTitle(glance, (await folderLabel(folder)).label),
    { openFolder, confirmOverwrite },
  )
}

const terminalPid = (terminal: vscode.Terminal): Promise<number | undefined> =>
  Promise.race([
    Promise.resolve(terminal.processId),
    new Promise<undefined>((resolve) =>
      setTimeout(() => resolve(undefined), TERMINAL_PID_MS),
    ),
  ])

const countRunning = async (): Promise<{
  activeSessions: number
  busyTerminals: number
}> => {
  const parents = await readParents()
  const pids = await Promise.all(vscode.window.terminals.map(terminalPid))
  const busyTerminals = pids.filter(
    (pid) => pid === undefined || hasChildren(pid, parents),
  ).length
  const ids = sessionIdsUnder(await readLiveSessions(), process.pid, parents)
  const activeSessions = countActive(ids, await readStatuses())
  return { activeSessions, busyTerminals }
}

/** What reopening this window now would lose, in words; empty when safe. */
const reasonsNotToReopen = async (): Promise<string[]> => {
  const running = await countRunning()
    .then((counts) => ({ ...counts, checkFailed: false }))
    .catch((error: unknown) => {
      log(error)
      return { activeSessions: 0, busyTerminals: 0, checkFailed: true }
    })
  return unsafeReasons({
    ...running,
    dirtyDocuments: vscode.workspace.textDocuments.filter((doc) => doc.isDirty)
      .length,
  })
}

const autoReopenEnabled = (): boolean =>
  vscode.workspace
    .getConfiguration('aydy')
    .get<boolean>(AUTO_REOPEN_SETTING, true)

const settle = (): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, SETTLE_MS))

/**
 * Reopens a plain single-folder window as its workspace file without asking,
 * when nothing in it would be lost. Otherwise it says what is in the way and
 * leaves the choice to the user.
 */
const autoReopen = async (folder: string): Promise<void> => {
  if (!autoReopenEnabled()) {
    return
  }
  await settle()
  if (openWorkspaceFile() !== undefined) {
    return
  }
  const reasons = await reasonsNotToReopen()
  if (reasons.length === 0) {
    await openWorkspace(folder)
    return
  }
  const choice = await vscode.window.showInformationMessage(
    `AYDY shows how many Claude Code sessions need you in this window’s tab by reopening it as a workspace. It did not reopen it automatically because this window has ${listReasons(reasons)}.`,
    NOT_NOW,
    REOPEN_ANYWAY,
  )
  if (choice === REOPEN_ANYWAY) {
    await openWorkspace(folder)
  }
}

/**
 * Moves a window opened from a workspace file in AYDY's data folder onto the
 * same file beside the folder, when that spot is free or already AYDY's and
 * nothing in the window would be lost. A spot taken by your own file is left
 * alone, so this never asks.
 */
const migrateLegacy = async (folder: string, legacy: string): Promise<void> => {
  if (!autoReopenEnabled()) {
    return
  }
  const sibling = siblingWorkspacePath(folder)
  const state = await workspaceState(sibling, folder)
  if (state === 'foreign') {
    return
  }
  await settle()
  if ((await reasonsNotToReopen()).length > 0) {
    return
  }
  if (state === 'free') {
    await writeWorkspace(
      sibling,
      folder,
      tabTitle(glance, (await folderLabel(folder)).label),
    )
  }
  await registerWorkspace(sibling, folder)
  await workspaceWrites
  await openFolder(sibling, { forceReuseWindow: true })
  console.info(`AYDY: moved ${legacy} to ${sibling}`)
}

const showCountInTabTitle = async (folders: string[]): Promise<void> => {
  const folder = folders[0]
  if (
    folder === undefined ||
    folders.length !== 1 ||
    openWorkspaceFile() !== undefined
  ) {
    await vscode.window.showInformationMessage(SINGLE_FOLDER_ONLY)
    return
  }
  await openWorkspace(folder)
}

/** The once-per-folder offer flags earlier versions kept; now unused. */
const forgetOfferFlags = async (
  context: vscode.ExtensionContext,
): Promise<void> => {
  for (const key of context.globalState.keys()) {
    if (key.startsWith('aydy.reopenOffered:')) {
      await context.globalState.update(key, undefined)
    }
  }
}

/** Decides what this window does with workspace files as it starts. */
const startWorkspace = async (
  context: vscode.ExtensionContext,
  folders: string[],
): Promise<void> => {
  const current = openWorkspaceFile()
  if (current === undefined) {
    const folder = folders[0]
    if (folder !== undefined && folders.length === 1) {
      await autoReopen(folder)
    }
    return
  }
  const folder = await readWorkspaceFolder(current)
  if (folder === null) {
    return
  }
  await keepGeneratedWorkspace(context, current, folder)
  if (isLegacyWorkspace(current)) {
    await migrateLegacy(folder, current)
  } else {
    await registerWorkspace(current, folder)
    await removeLegacyWorkspace(folder)
  }
}

const localFolders = (): string[] =>
  (vscode.workspace.workspaceFolders ?? [])
    .filter((folder) => folder.uri.scheme === 'file')
    .map((folder) => {
      try {
        return realpathSync(folder.uri.fsPath)
      } catch {
        return folder.uri.fsPath
      }
    })

const publish = (folders: string[]): void => {
  publishWindow(process.pid, folders).catch(() => undefined)
}

const unpublish = async (): Promise<void> => {
  await unpublishWindow(process.pid).catch(() => undefined)
}

/**
 * Everything a reveal reads and does, gathered off the `vscode` API.
 *
 * The decisions behind it — which version is trusted, whether the tab is here,
 * whether one opened that should not have — all live in `revealSession.ts`,
 * where they can be driven without an editor.
 */
const revealEnvironment: RevealEnvironment = {
  claudeCodeVersion,
  claudeTabs: () =>
    vscode.window.tabGroups.all.flatMap((group) =>
      group.tabs.filter(isClaudeTab).map((tab) => tab.label),
    ),
  // The argument order Claude's own webview uses: the session, no prompt to
  // send, and the column already in front. That third argument is not only
  // about where the tab lands — the command rewrites the user's preferred
  // location to the panel for any other value, and a jump has no business
  // changing a setting.
  open: async (sessionId: string) => {
    await vscode.commands.executeCommand(
      REVEAL_COMMAND,
      sessionId,
      undefined,
      vscode.ViewColumn.Active,
    )
  },
  watchOpenedTabs: (opened: () => void) =>
    vscode.window.tabGroups.onDidChangeTabs((change) => {
      if (change.opened.some(isClaudeTab)) {
        opened()
      }
    }),
}

const OPEN_FILE = 'Open file'
const TRY_AGAIN = 'Try again'

const installTracking = async (
  context: vscode.ExtensionContext,
): Promise<void> => {
  try {
    await installHookScript(context.extensionPath)
    const result = await ensureHooks()
    if (result.ok) {
      await releaseFailureNotice()
      return
    }
    if (!(await claimFailureNotice())) {
      return
    }
    const settingsPath = claudeSettingsPath()
    const choice = await vscode.window.showErrorMessage(
      failureMessage(result.reason, settingsPath),
      OPEN_FILE,
      TRY_AGAIN,
    )
    if (choice === OPEN_FILE) {
      await vscode.window.showTextDocument(vscode.Uri.file(settingsPath))
    } else if (choice === TRY_AGAIN) {
      await releaseFailureNotice()
      await installTracking(context)
    }
  } catch {
    // Tracking is best effort; activation must go on without it.
  }
}

/**
 * Installs tracking, publishes this window's folders, keeps the tab count and
 * title, and answers jump requests, all through files under AYDY's data
 * directory. It needs no running app and asks nothing.
 */
export const activate = (context: vscode.ExtensionContext): void => {
  void installTracking(context)

  let folders = localFolders()
  publish(folders)
  const tracker = new WindowTracker({
    pid: process.pid,
    folders: () => folders,
    focused: () => vscode.window.state.focused,
    onGlance: (next) => {
      glance = next
      writeTitle()
    },
  })

  const jumps = new JumpResponder({
    dir: jumpDir(),
    pid: process.pid,
    folders: () => folders,
    open: () => {
      const file = vscode.workspace.workspaceFile
      return file?.scheme === 'file' ? file.fsPath : null
    },
    reveal: async (request) => revealSession(request, revealEnvironment),
  })

  startWorkspace(context, folders).catch(log)
  forgetOfferFlags(context).catch(log)
  sweepWorkspaces().catch(log)

  context.subscriptions.push(
    { dispose: () => void unpublish() },
    tracker,
    jumps,
    vscode.commands.registerCommand('aydy.showSessionCountInTabTitle', () =>
      showCountInTabTitle(folders).catch(log),
    ),
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      folders = localFolders()
      publish(folders)
      tracker.poke()
    }),
    vscode.window.onDidChangeWindowState(() => {
      tracker.poke()
    }),
  )

  void tracker.start()
  void jumps.start()
}

/**
 * Withdraws this window's published folder file, so other windows stop counting
 * it as an owner.
 */
export const deactivate = async (): Promise<void> => {
  await unpublish()
}
