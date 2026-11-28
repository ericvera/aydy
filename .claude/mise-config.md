# Mise Configuration

## Quality commands

- Check:
  - yarn build-tsc
  - yarn lint
  - cargo clippy (from `src-tauri/`)
- Unit tests:
  - yarn test
  - cargo test (from `src-tauri/`)

## Mock conditions

- Any UI change

## Skills & guides

- docs/design/ui-conventions.md (doc, required): before writing or editing any Vue component or view — create it from Mock guidance if it does not exist yet
- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Cleanup

1. Squash-merge this branch's pull request: `gh pr merge --squash`.
2. Sync main: `git switch main && git pull --ff-only origin main`.
3. Delete the branch locally and on the remote: `git branch -D <branch>`, `git push origin --delete <branch>` (skip the remote delete if it is already gone), and `git update-ref -d refs/review/<branch>` if that ref exists.
4. Install dependencies: `yarn install`.
5. Deploy the app: `yarn tauri build`, quit the running app (`osascript -e 'quit app "AYDY"'`), replace `/Applications/AYDY.app` with `src-tauri/target/release/bundle/macos/AYDY.app`, then `open /Applications/AYDY.app`.
6. Deploy the extension: `yarn package-extension`, then `code --install-extension` the newest `extension/aydy-companion-*.vsix`.
