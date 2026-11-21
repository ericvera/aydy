import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import type { WorkspaceDirs } from './workspaceFile'
import {
  buildWorkspace,
  isLegacyWorkspace,
  jsonEqual,
  legacyWorkspacePath,
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

let root: string
let dirs: WorkspaceDirs
let folder: string

beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'aydy-workspace-')))
  dirs = { workspaces: join(root, 'workspaces') }
  folder = join(root, 'okven')
  mkdirSync(join(folder, '.vscode'), { recursive: true })
})

afterEach(() => {
  rmSync(root, { force: true, recursive: true })
})

const write = async (title: string | null): Promise<string> => {
  const path = siblingWorkspacePath(folder)
  await writeWorkspace(path, folder, title)
  return path
}

const writeFolderSettings = (text: string): void => {
  writeFileSync(join(folder, '.vscode', 'settings.json'), text)
}

const readJson = (path: string): Record<string, unknown> =>
  JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>

const editFile = (
  path: string,
  edit: (doc: Record<string, unknown>) => void,
): void => {
  const doc = readJson(path)
  edit(doc)
  writeFileSync(path, JSON.stringify(doc, null, 2))
}

it('siblingWorkspacePath: sits beside the folder, named after it', () => {
  expect(siblingWorkspacePath('/Users/me/Code/okven')).toBe(
    '/Users/me/Code/okven.code-workspace',
  )
  expect(siblingWorkspacePath('/Users/me/Code/okven.worktrees/feat-x')).toBe(
    '/Users/me/Code/okven.worktrees/feat-x.code-workspace',
  )
})

it('legacyWorkspacePath: is stable and lives under a 12-hex hash dir', () => {
  const first = legacyWorkspacePath(folder, dirs)
  expect(legacyWorkspacePath(folder, dirs)).toBe(first)
  expect(first).toMatch(
    new RegExp(`^${dirs.workspaces}/[0-9a-f]{12}/okven\\.code-workspace$`),
  )
  expect(legacyWorkspacePath(join(root, 'other'), dirs)).not.toBe(first)
})

it('writeWorkspace: writes folders, copied settings, the title and the aydy record', async () => {
  writeFolderSettings(
    '{\n  // comment\n  "editor.tabSize": 4,\n  "files.exclude": {"out": true},\n  "window.title": "repo title",\n}',
  )
  const path = await write('○ okven')
  const text = readFileSync(path, 'utf8')
  expect(text.endsWith('}\n')).toBe(true)
  expect(text).toContain('\n  "folders"')
  expect(JSON.parse(text)).toEqual({
    folders: [{ path: folder }],
    settings: {
      'editor.tabSize': 4,
      'files.exclude': { out: true },
      'window.title': '○ okven',
    },
    aydy: {
      folder,
      copiedSettings: { 'editor.tabSize': 4, 'files.exclude': { out: true } },
    },
  })
})

it('writeWorkspace: a missing folder settings file copies nothing', async () => {
  const path = await write(null)
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: {},
    aydy: { folder, copiedSettings: {} },
  })
})

it('syncFolderSettings: removes deleted keys, keeps user-added keys and keeps the title', async () => {
  writeFolderSettings('{"a": 1, "b": 2}')
  const path = await write('● okven')
  editFile(path, (doc) => {
    ;(doc['settings'] as Record<string, unknown>)['user.added'] = true
  })
  writeFolderSettings('{"a": 1}')
  await syncFolderSettings(path)
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: { a: 1, 'user.added': true, 'window.title': '● okven' },
    aydy: { folder, copiedSettings: { a: 1 } },
  })
})

it('syncFolderSettings: keeps folders the user added, primary first', async () => {
  const path = await write(null)
  editFile(path, (doc) => {
    doc['folders'] = [{ path: '/other' }, { path: folder, name: 'Mine' }, 7]
  })
  await syncFolderSettings(path)
  expect(readJson(path)['folders']).toEqual([
    { path: folder, name: 'Mine' },
    { path: '/other' },
  ])
})

it('buildWorkspace: keeps an existing [primary, other] folder list intact', () => {
  const built = buildWorkspace(
    { folders: [{ path: folder }, { path: '/other' }] },
    folder,
    {},
    null,
  )
  expect(built['folders']).toEqual([{ path: folder }, { path: '/other' }])
})

it('syncFolderSettings: keeps user edits to copied keys and updates unedited ones', async () => {
  writeFolderSettings('{"edited": 1, "dropped": 1, "plain": 1}')
  const path = await write(null)
  editFile(path, (doc) => {
    const settings = doc['settings'] as Record<string, unknown>
    settings['edited'] = 'user'
    settings['dropped'] = 'user'
  })
  writeFolderSettings('{"edited": 2, "plain": 2}')
  await syncFolderSettings(path)
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: { edited: 'user', dropped: 'user', plain: 2 },
    aydy: { folder, copiedSettings: { plain: 2 } },
  })
})

it('jsonEqual: ignores key order and compares structurally', () => {
  expect(
    jsonEqual({ a: 1, b: { c: [1, 2] } }, { b: { c: [1, 2] }, a: 1 }),
  ).toBe(true)
  expect(jsonEqual({ a: 1 }, { a: 1, b: undefined })).toBe(false)
  expect(jsonEqual([1, 2], [2, 1])).toBe(false)
  expect(jsonEqual({ a: [1] }, { a: { 0: 1 } })).toBe(false)
  expect(jsonEqual(null, {})).toBe(false)
  expect(jsonEqual('x', 'x')).toBe(true)
})

it('setWorkspaceTitle: writes a changed title and skips an unchanged one', async () => {
  const path = await write('○ okven')
  const past = new Date(Date.now() - 60_000)
  utimesSync(path, past, past)
  const before = statSync(path).mtimeMs
  expect(await setWorkspaceTitle(path, '○ okven')).toBe(false)
  expect(statSync(path).mtimeMs).toBe(before)
  expect(await setWorkspaceTitle(path, '● okven')).toBe(true)
  expect(
    (readJson(path)['settings'] as Record<string, unknown>)['window.title'],
  ).toBe('● okven')
})

it('syncFolderSettings: a broken folder settings file keeps the prior copy', async () => {
  writeFolderSettings('{"a": 1}')
  const path = await write('○ okven')
  writeFolderSettings('{"a": 2, "b":')
  await syncFolderSettings(path)
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: { a: 1, 'window.title': '○ okven' },
    aydy: { folder, copiedSettings: { a: 1 } },
  })
})

it('isLegacyWorkspace: is true only for paths under the workspaces dir', () => {
  expect(isLegacyWorkspace(legacyWorkspacePath(folder, dirs), dirs)).toBe(true)
  expect(isLegacyWorkspace(dirs.workspaces, dirs)).toBe(false)
  expect(isLegacyWorkspace(`${dirs.workspaces}-x/a.code-workspace`, dirs)).toBe(
    false,
  )
  expect(isLegacyWorkspace(join(root, 'a.code-workspace'), dirs)).toBe(false)
})

it('workspaceState: tells a missing, an AYDY and a foreign file apart', async () => {
  const path = siblingWorkspacePath(folder)
  expect(await workspaceState(path, folder)).toBe('free')
  await write(null)
  expect(await workspaceState(path, folder)).toBe('ours')
  expect(await workspaceState(path, join(root, 'moved'))).toBe('foreign')
  writeFileSync(path, '{"folders": [{"path": "."}]}')
  expect(await workspaceState(path, folder)).toBe('foreign')
})

it('writeWorkspace: fresh replaces a foreign file instead of merging into it', async () => {
  const path = siblingWorkspacePath(folder)
  writeFileSync(
    path,
    '{"folders": [{"path": "/x"}], "settings": {"mine": true}, "extensions": {}}',
  )
  await writeWorkspace(path, folder, '○ okven', { fresh: true })
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: { 'window.title': '○ okven' },
    aydy: { folder, copiedSettings: {} },
  })
})

it('sweepWorkspaces: removes a registered file whose folder is gone, and only that', async () => {
  const kept = await write(null)
  await registerWorkspace(kept, folder, dirs)
  const gone = join(root, 'feat-x')
  mkdirSync(gone)
  const stale = siblingWorkspacePath(gone)
  await writeWorkspace(stale, gone, null)
  await registerWorkspace(stale, gone, dirs)
  rmSync(gone, { recursive: true })

  expect(await sweepWorkspaces(dirs)).toEqual([stale])
  expect(existsSync(stale)).toBe(false)
  expect(existsSync(kept)).toBe(true)
  expect(readdirSync(join(dirs.workspaces, 'index'))).toHaveLength(1)
})

it("sweepWorkspaces: leaves a file that is no longer AYDY's for that folder", async () => {
  const gone = join(root, 'feat-x')
  mkdirSync(gone)
  const path = siblingWorkspacePath(gone)
  await writeWorkspace(path, gone, null)
  await registerWorkspace(path, gone, dirs)
  writeFileSync(path, '{"folders": []}')
  rmSync(gone, { recursive: true })

  expect(await sweepWorkspaces(dirs)).toEqual([])
  expect(existsSync(path)).toBe(true)
  expect(readdirSync(join(dirs.workspaces, 'index'))).toHaveLength(0)
})

it("sweepWorkspaces: keeps a file whose folder's parent is missing, like an unmounted volume", async () => {
  const volume = join(root, 'Volumes', 'ext')
  const repo = join(volume, 'repo')
  mkdirSync(repo, { recursive: true })
  const path = legacyWorkspacePath(repo, dirs)
  await writeWorkspace(path, repo, null)
  rmSync(volume, { recursive: true })

  expect(await sweepWorkspaces(dirs)).toEqual([])
  expect(existsSync(path)).toBe(true)
})

it('sweepWorkspaces: removes a legacy file whose folder is gone', async () => {
  const gone = join(root, 'feat-x')
  mkdirSync(gone)
  const path = legacyWorkspacePath(gone, dirs)
  await writeWorkspace(path, gone, null)
  const live = legacyWorkspacePath(folder, dirs)
  await writeWorkspace(live, folder, null)
  rmSync(gone, { recursive: true })

  expect(await sweepWorkspaces(dirs)).toEqual([path])
  expect(existsSync(path)).toBe(false)
  expect(existsSync(live)).toBe(true)
})

it("removeLegacyWorkspace: removes only AYDY's legacy file for that folder", async () => {
  const path = legacyWorkspacePath(folder, dirs)
  await writeWorkspace(path, folder, null)
  await removeLegacyWorkspace(join(root, 'other'), dirs)
  expect(existsSync(path)).toBe(true)
  await removeLegacyWorkspace(folder, dirs)
  expect(existsSync(path)).toBe(false)
})

it('readWorkspaceFolder: returns aydy.folder or null', async () => {
  const path = await write(null)
  expect(await readWorkspaceFolder(path)).toBe(folder)
  expect(await readWorkspaceFolder(join(root, 'missing.code-workspace'))).toBe(
    null,
  )
  const broken = join(root, 'broken.code-workspace')
  writeFileSync(broken, '{"aydy": ')
  expect(await readWorkspaceFolder(broken)).toBe(null)
  const plain = join(root, 'plain.code-workspace')
  writeFileSync(plain, '{"folders": [{"path": "/x"}]}')
  expect(await readWorkspaceFolder(plain)).toBe(null)
})
