import { realpathSync } from 'node:fs'
import { basename } from 'node:path'

import * as vscode from 'vscode'

import type { WindowFacts } from './aydyClient'
import { AydyClient, aydyPortFilePath, versionOf } from './aydyClient'
import { claudeSettingsPath, jumpDir } from './aydyPaths'
import type { FocusState } from './focusReporter'
import { FocusReporter } from './focusReporter'
import {
  claimFailureNotice,
  failureMessage,
  releaseFailureNotice,
} from './hookNotice'
import { ensureHooks, installHookScript } from './hookInstall'
import { JumpResponder } from './jumpRequests'
import { offerKey, reopenAsWorkspace, shouldOffer } from './reopenOffer'
import type { RevealEnvironment, RevealRequest } from './revealSession'
import { revealSession } from './revealSession'
import { tabTitle } from './sessionState'
import { publishWindow, unpublishWindow } from './windowRegistry'
import { WindowTracker } from './windowTracker'
import {
  isGeneratedWorkspace,
  readWorkspaceFolder,
  setWorkspaceTitle,
  syncFolderSettings,
} from './workspaceFile'

/**
 * The Claude Code extension, whose version this integration is pinned against.
 *
 * It exports no API, so nothing here calls it; the version is reported to AYDY
 * so that a window running one this integration has not been checked against is
 * a fact AYDY holds rather than a mystery in a bug report.
 */
const CLAUDE_CODE_EXTENSION = 'Anthropic.claude-code'

/**
 * The webview panel Claude Code opens a session in, which is what a tab showing
 * a session is made of.
 *
 * The editor prefixes the view type an extension chose, so this is tested by
 * containment rather than equality. Getting it wrong leaves every tab looking
 * like no Claude tab at all, which suppresses nothing — the recoverable
 * direction — whereas reporting any tab's label invites a source file named
 * like a session summary to clear an unread the user never saw.
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

let client: AydyClient | undefined

let unreadCount = 0

export const currentUnreadCount = (): number => unreadCount

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
      tabTitle(unreadCount, basename(target.folder)),
    ),
  )
}

const syncSettings = (path: string): void => {
  enqueueWorkspaceWrite(() => syncFolderSettings(path))
}

const generatedWorkspaceFile = (): string | undefined => {
  const file = vscode.workspace.workspaceFile
  return file?.scheme === 'file' && isGeneratedWorkspace(file.fsPath)
    ? file.fsPath
    : undefined
}

const keepGeneratedWorkspace = async (
  context: vscode.ExtensionContext,
  path: string,
): Promise<void> => {
  const folder = await readWorkspaceFolder(path)
  if (folder === null) {
    return
  }
  generated = { path, folder }
  syncSettings(path)
  writeTitle()
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

const REOPEN_MESSAGE =
  "Show how many Claude Code sessions need you in this window's tab? AYDY reopens this window from a workspace file kept outside the repo. This restarts the window, which may end Claude Code sessions open in it."
const NOT_NOW = 'Not now'
const REOPEN = 'Reopen as workspace'
const SINGLE_FOLDER_ONLY =
  'AYDY: The session count in the tab title only applies to a window opened as a single folder.'

const openWorkspace = async (folder: string): Promise<void> => {
  await reopenAsWorkspace(folder, tabTitle(unreadCount, basename(folder)), {
    openFolder: (path, options) =>
      vscode.commands.executeCommand(
        'vscode.openFolder',
        vscode.Uri.file(path),
        options,
      ),
  })
}

const workspaceFileName = (): string | undefined =>
  vscode.workspace.workspaceFile?.toString()

const offerReopen = async (
  context: vscode.ExtensionContext,
  folders: string[],
): Promise<void> => {
  const folder = folders[0]
  if (
    folder === undefined ||
    !shouldOffer({
      workspaceFile: workspaceFileName(),
      folders,
      offered: context.globalState.get<boolean>(offerKey(folder)) === true,
    })
  ) {
    return
  }
  // Marked before asking, so a reload during the notice cannot show it twice.
  await context.globalState.update(offerKey(folder), true)
  const choice = await vscode.window.showInformationMessage(
    REOPEN_MESSAGE,
    NOT_NOW,
    REOPEN,
  )
  if (choice === REOPEN) {
    await openWorkspace(folder)
  }
}

const showCountInTabTitle = async (folders: string[]): Promise<void> => {
  const folder = folders[0]
  if (
    folder === undefined ||
    !shouldOffer({
      workspaceFile: workspaceFileName(),
      folders,
      offered: false,
    })
  ) {
    await vscode.window.showInformationMessage(SINGLE_FOLDER_ONLY)
    return
  }
  await openWorkspace(folder)
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

const windowFacts = (): WindowFacts => ({
  pid: process.pid,
  workspaceFolders: (vscode.workspace.workspaceFolders ?? []).map(
    (folder) => folder.uri.fsPath,
  ),
  claudeCodeVersion: claudeCodeVersion(),
})

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

/**
 * The label on this window's active Claude Code tab, or `null` when the tab in
 * front is not one.
 *
 * A session opened in the sidebar is a view rather than a tab and never appears
 * among the tab groups at all, so having no Claude tab is an ordinary state and
 * not a failure to read one.
 */
const activeTabLabel = (): string | null => {
  const tab = vscode.window.tabGroups.activeTabGroup.activeTab

  return tab !== undefined && isClaudeTab(tab) ? tab.label : null
}

const focusState = (): FocusState => ({
  focused: vscode.window.state.focused,
  activeTabLabel: activeTabLabel(),
})

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
 * Starts reporting this window to AYDY.
 *
 * There is nothing to configure and nothing to ask: AYDY publishes its port and
 * this window finds it. AYDY not being installed is the ordinary case on most
 * machines, and leaves this doing nothing visible at all.
 */
export const activate = (context: vscode.ExtensionContext): void => {
  void installTracking(context)

  const started = new AydyClient({
    facts: windowFacts(),
    portFilePath: aydyPortFilePath(),
    // An AYDY that has just registered this window knows nothing else about
    // it, and where the user is is only ever reported on change. A window
    // sitting untouched while AYDY restarts would otherwise stay unaccounted
    // for until the user moved something.
    onRegistered: () => {
      focus.resend()
    },
    reveal: async (request: RevealRequest) =>
      revealSession(request, revealEnvironment),
  })
  const focus = new FocusReporter({
    send: (state) => started.reportFocus(state.focused, state.activeTabLabel),
  })
  const reportFocus = (): void => {
    focus.report(focusState())
  }

  let folders = localFolders()
  publish(folders)
  const tracker = new WindowTracker({
    pid: process.pid,
    folders: () => folders,
    focused: () => vscode.window.state.focused,
    onCount: (count) => {
      unreadCount = count
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

  const generatedPath = generatedWorkspaceFile()
  if (generatedPath === undefined) {
    offerReopen(context, folders).catch(log)
  } else {
    keepGeneratedWorkspace(context, generatedPath).catch(log)
  }

  client = started
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
    {
      dispose: () => {
        focus.dispose()
        void started.close()
      },
    },
    // Folders come and go from a multi-root workspace without the extension
    // host restarting, so what was open at activation is only right until the
    // user adds or removes one — and AYDY picks the window to send a session
    // to from exactly this list.
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      void started.update(windowFacts())
    }),
    // Focus is half of what says whether the user is watching a session, and
    // losing it matters as much as gaining it: a window AYDY still believes is
    // in front goes on silencing its own notifications.
    vscode.window.onDidChangeWindowState(reportFocus),
    // The tab in front is the other half. It moves when a tab is opened,
    // closed or activated, and when a different group becomes the active one.
    vscode.window.tabGroups.onDidChangeTabs(reportFocus),
    vscode.window.tabGroups.onDidChangeTabGroups(reportFocus),
  )

  void started.start()
  void tracker.start()
  void jumps.start()
  reportFocus()
}

/** Tells AYDY this window has gone, rather than leaving it to time out. */
export const deactivate = async (): Promise<void> => {
  await unpublish()
  await client?.close()
  client = undefined
}
