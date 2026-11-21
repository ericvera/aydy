import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import type { SessionGroupKind } from '../../types/SessionGroupKind'
import GroupHeader from './GroupHeader.vue'

const header = (
  kind: SessionGroupKind,
  repo: string | null,
  branch: string | null,
): string =>
  renderedText(mount(GroupHeader, { props: { kind, repo, branch } }).element)

it('writes a project group as repo then branch', () => {
  expect(header('project', 'okven', 'eric/attribution-2')).toBe(
    'okven · eric/attribution-2',
  )
})

it('writes the repository alone while its branch is unknown', () => {
  expect(header('project', 'aydy', null)).toBe('aydy')
})

it('names the group for sessions whose project is not known yet', () => {
  expect(header('unidentified', null, null)).toBe('Not yet identified')
})

it('names the group for sessions that cannot be jumped to', () => {
  expect(header('notJumpable', null, null)).toBe('Not jumpable')
})

/**
 * The not-jumpable group collects sessions from other clients and other
 * machines, which may well carry a repository and a branch. It is still the
 * not-jumpable group, and saying so is what stops a user expecting a click to
 * take them there.
 */
it('still names the not-jumpable group when its sessions have a repository', () => {
  expect(header('notJumpable', 'okven', 'main')).toBe('Not jumpable')
})

/**
 * A group header does not indent its rows: indentation was tried during mock
 * review and rejected. It is a full-width line at the list's own gutter.
 */
it('draws the header at the list gutter', () => {
  const wrapper = mount(GroupHeader, {
    props: { kind: 'project', repo: 'aydy', branch: 'main' },
  })

  expect(wrapper.classes().sort()).toMatchInlineSnapshot(`
    [
      "font-3",
      "pb-2",
      "pt-4",
      "px-5",
      "text-0",
      "text-neutral-6",
      "uppercase",
    ]
  `)
})
