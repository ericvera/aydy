# Mise Configuration

Mise directory: .mise/
Branch convention: feat/<slug> for features, fix/<slug> for bug fixes
Ship: merge (squash)

## Quality commands

- Format: handled by the lint-staged pre-commit hook (prettier on staged JS/TS/Vue, rustfmt on staged Rust); no repo-wide format run
- Check:
  - yarn build-tsc
  - yarn lint
  - cargo clippy (from `src-tauri/`)
- Unit tests:
  - yarn test
  - cargo test (from `src-tauri/`)

## Mock conditions

- Any UI change

## Mock guidance

Product: AYDY. UI code root: `src/`. macOS menu bar popover — compact, native-feeling, **dark mode only** (no light theme). English copy. Mocks follow docs/design/ui-conventions.md: strict design tokens only (no arbitrary values), atoms/molecules/organisms layering, typography atoms over raw tags, bold for emphasis — never quote characters.

## Test conventions

Rust: `#[cfg(test)]` modules colocated with the source; integration tests in `src-tauri/tests/`. Vue/TS: vitest, colocated `*.test.ts` next to the source file.

## Test exceptions

- Purely visual changes (spacing, colors, styles) — verify with screenshots
- Anything that would need an e2e test (no e2e infrastructure for a menu bar app) — verify with unit tests plus manual verification

## Skills & guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Models

- implementer: opus
- explore: opus
- retrospective: opus
