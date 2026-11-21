import { expect, it } from 'vitest'

import { parseJsonc } from './jsonc'

it('parseJsonc: strips line comments', () => {
  expect(parseJsonc('{\n  // note\n  "a": 1 // trailing\n}')).toEqual({ a: 1 })
})

it('parseJsonc: strips block comments, including multi-line ones', () => {
  expect(
    parseJsonc('/* head */{"a": /* inline */ 1,\n/*\n x\n*/"b": 2}'),
  ).toEqual({ a: 1, b: 2 })
})

it('parseJsonc: keeps comment markers inside strings', () => {
  expect(
    parseJsonc('{"url": "https://example.com", "glob": "/* not a comment */"}'),
  ).toEqual({ url: 'https://example.com', glob: '/* not a comment */' })
})

it('parseJsonc: keeps escaped quotes and backslashes inside strings', () => {
  expect(
    parseJsonc('{"q": "say \\"hi\\" // still string", "b": "end\\\\"} // c'),
  ).toEqual({ q: 'say "hi" // still string', b: 'end\\' })
})

it('parseJsonc: removes trailing commas before } and ]', () => {
  expect(parseJsonc('{"a": [1, 2, ], "b": {"c": 3,},\n}')).toEqual({
    a: [1, 2],
    b: { c: 3 },
  })
})

it('parseJsonc: keeps a comma followed by a comment and a value', () => {
  expect(parseJsonc('[1, // one\n 2]')).toEqual([1, 2])
})

it('parseJsonc: keeps commas inside strings that precede a bracket', () => {
  expect(parseJsonc('{"a": ",}"}')).toEqual({ a: ',}' })
})

it('parseJsonc: throws on invalid input', () => {
  expect(() => parseJsonc('{"a": }')).toThrow()
  expect(() => parseJsonc('{"a": 1')).toThrow()
  expect(() => parseJsonc('')).toThrow()
  expect(() => parseJsonc('{"a": "unterminated}')).toThrow()
})
