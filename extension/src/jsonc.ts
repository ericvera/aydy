const stringEnd = (text: string, start: number): number => {
  let index = start + 1
  while (index < text.length && text[index] !== '"') {
    index += text[index] === '\\' ? 2 : 1
  }
  return index + 1
}

const stripComments = (text: string): string => {
  let out = ''
  let index = 0
  while (index < text.length) {
    const char = text[index]
    const next = text[index + 1]
    if (char === '"') {
      const end = stringEnd(text, index)
      out += text.slice(index, end)
      index = end
    } else if (char === '/' && next === '/') {
      const end = text.indexOf('\n', index)
      index = end === -1 ? text.length : end
    } else if (char === '/' && next === '*') {
      const end = text.indexOf('*/', index + 2)
      if (end === -1) {
        throw new SyntaxError('Unterminated block comment')
      }
      out += ' '
      index = end + 2
    } else {
      out += char
      index += 1
    }
  }
  return out
}

const closesNext = (text: string, from: number): boolean => {
  let index = from
  while (index < text.length && /\s/.test(text[index] ?? '')) {
    index += 1
  }
  return text[index] === '}' || text[index] === ']'
}

const stripTrailingCommas = (text: string): string => {
  let out = ''
  let index = 0
  while (index < text.length) {
    const char = text[index]
    if (char === '"') {
      const end = stringEnd(text, index)
      out += text.slice(index, end)
      index = end
    } else if (char === ',' && closesNext(text, index + 1)) {
      index += 1
    } else {
      out += char
      index += 1
    }
  }
  return out
}

export const parseJsonc = (text: string): unknown =>
  JSON.parse(stripTrailingCommas(stripComments(text)))
