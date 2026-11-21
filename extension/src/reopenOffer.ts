import type { WorkspaceDirs } from './workspaceFile'
import { writeWorkspace } from './workspaceFile'

export interface OfferState {
  workspaceFile?: string | undefined
  folders: string[]
  offered: boolean
}

export interface ReopenEnvironment {
  openFolder(
    path: string,
    options: { forceReuseWindow: boolean },
  ): Thenable<unknown>
  dirs?: WorkspaceDirs
}

export const offerKey = (folder: string): string =>
  `aydy.reopenOffered:${folder}`

export const shouldOffer = (state: OfferState): boolean =>
  state.workspaceFile === undefined &&
  state.folders.length === 1 &&
  !state.offered

export const reopenAsWorkspace = async (
  folder: string,
  title: string,
  env: ReopenEnvironment,
): Promise<void> => {
  const path = await writeWorkspace(folder, title, env.dirs)
  await env.openFolder(path, { forceReuseWindow: true })
}
