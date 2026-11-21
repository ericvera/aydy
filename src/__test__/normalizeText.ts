/**
 * Collapses runs of whitespace to a single space and trims the result, so
 * rendered template text can be asserted against a plain string regardless of
 * how the template was indented.
 */
export const normalizeText = (value: string): string =>
  value.replace(/\s+/g, ' ').trim()
