import { normalizeText } from './normalizeText'

const TEXT_NODE = 3

// `hidden` is `display: none`, which no test environment applies because none
// of them load the generated stylesheet. Skipping it here is what keeps these
// assertions about what the popover shows: a row's hover detail sits in the
// DOM at all times so the browser can reveal it from `:hover` alone.
const isHidden = (node: Node): boolean =>
  node instanceof Element && node.classList.contains('hidden')

const collect = (node: Node, parts: string[]): void => {
  for (const child of Array.from(node.childNodes)) {
    if (child.nodeType === TEXT_NODE) {
      const value = normalizeText(child.nodeValue ?? '')

      if (value !== '') {
        parts.push(value)
      }

      continue
    }

    if (isHidden(child)) {
      continue
    }

    collect(child, parts)
  }
}

/**
 * The text a component renders, with a space at every element boundary, and
 * without anything the design hides.
 *
 * `wrapper.text()` concatenates sibling elements with nothing between them, so
 * two adjacent spans come back as one run-together word. The popover puts each
 * part of a row in its own element, spaced by a flex gap, so that is most of
 * what there is to assert.
 *
 * The element passed in is read whether it is hidden or not; only hidden
 * elements inside it are skipped, so a hidden part can still be asserted on
 * directly.
 */
export const renderedText = (element: Element): string => {
  const parts: string[] = []

  collect(element, parts)

  return parts.join(' ')
}
