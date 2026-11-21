import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

import type { WorkspaceDirs } from './workspaceFile'
import {
  buildWorkspace,
  generatedWorkspacePath,
  isGeneratedWorkspace,
  jsonEqual,
  readWorkspaceFolder,
  setWorkspaceTitle,
  syncFolderSettings,
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

it('generatedWorkspacePath: is stable and lives under a 12-hex hash dir', () => {
  const first = generatedWorkspacePath(folder, dirs)
  expect(generatedWorkspacePath(folder, dirs)).toBe(first)
  expect(first).toMatch(
    new RegExp(`^${dirs.workspaces}/[0-9a-f]{12}/okven\\.code-workspace$`),
  )
  expect(generatedWorkspacePath(join(root, 'other'), dirs)).not.toBe(first)
})

it('writeWorkspace: hashes the realpath so a symlinked folder maps to one file', async () => {
  const link = join(root, 'link-to-okven')
  symlinkSync(folder, link)
  const direct = await writeWorkspace(folder, null, dirs)
  const viaLink = await writeWorkspace(link, null, dirs)
  expect(viaLink).toBe(direct)
  expect(direct).toBe(generatedWorkspacePath(folder, dirs))
})

it('writeWorkspace: writes folders, copied settings, the title and the aydy record', async () => {
  writeFolderSettings(
    '{\n  // comment\n  "editor.tabSize": 4,\n  "files.exclude": {"out": true},\n  "window.title": "repo title",\n}',
  )
  const path = await writeWorkspace(folder, '○ okven', dirs)
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
  const path = await writeWorkspace(folder, null, dirs)
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: {},
    aydy: { folder, copiedSettings: {} },
  })
})

it('syncFolderSettings: removes deleted keys, keeps user-added keys and keeps the title', async () => {
  writeFolderSettings('{"a": 1, "b": 2}')
  const path = await writeWorkspace(folder, '● okven', dirs)
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
  const path = await writeWorkspace(folder, null, dirs)
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
  const path = await writeWorkspace(folder, null, dirs)
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
  const path = await writeWorkspace(folder, '○ okven', dirs)
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
  const path = await writeWorkspace(folder, '○ okven', dirs)
  writeFolderSettings('{"a": 2, "b":')
  await syncFolderSettings(path)
  expect(readJson(path)).toEqual({
    folders: [{ path: folder }],
    settings: { a: 1, 'window.title': '○ okven' },
    aydy: { folder, copiedSettings: { a: 1 } },
  })
})

it('isGeneratedWorkspace: is true only for paths under the workspaces dir', () => {
  expect(isGeneratedWorkspace(generatedWorkspacePath(folder, dirs), dirs)).toBe(
    true,
  )
  expect(isGeneratedWorkspace(dirs.workspaces, dirs)).toBe(false)
  expect(
    isGeneratedWorkspace(`${dirs.workspaces}-x/a.code-workspace`, dirs),
  ).toBe(false)
  expect(isGeneratedWorkspace(join(root, 'a.code-workspace'), dirs)).toBe(false)
})

it('readWorkspaceFolder: returns aydy.folder or null', async () => {
  const path = await writeWorkspace(folder, null, dirs)
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
