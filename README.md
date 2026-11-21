# AYDY

A macOS menu bar app that shows every active Claude Code session at a glance — which ones
are working, which ones are waiting on you, and which ones are done.

AYDY lives in the menu bar only. It has no Dock icon and no app switcher entry.

## Stack

- **Tauri v2** (Rust core) — `src-tauri/`
- **Vue 3 + TypeScript** — `src/`
- Dark mode only, macOS only

## Prerequisites

- **macOS 13** or later
- **Node.js 24** or later
- **Yarn 4** — no global install needed; the repo pins its own release binary under
  `.yarn/releases/` and points at it from `.yarnrc.yml`
- **Rust toolchain** (stable) — install with [rustup](https://rustup.rs):
  `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Xcode Command Line Tools** — `xcode-select --install`

## Setup

```sh
yarn install
```

## Run

```sh
yarn tauri dev
```

This starts the Vite dev server and the Tauri shell together. Nothing appears in the Dock;
look in the menu bar.

## Build

```sh
yarn tauri build
```

The bundled app lands in `src-tauri/target/release/bundle/macos/AYDY.app`.

## The VS Code companion extension

`extension/` holds the VS Code extension AYDY needs to work at all. It installs the hook
script and Claude Code's hook entries that write each session's status file, so sessions
are tracked even while the app is closed, and it keeps each window's count of sessions
that need you in its tab title. Until it is installed the app shows only its Install the
extension screen. Package and install it from the repository root:

```sh
yarn package-extension
code --install-extension extension/aydy-companion-0.1.0.vsix
```

Installing it any other way — copying the folder into `~/.vscode/extensions` — leaves VS
Code's record of what is installed out of step with what is on disk.
[What it does](extension/README.md#what-it-does) in `extension/README.md` covers the files
the extension writes, and [Installing it](extension/README.md#installing-it) what that
mismatch looks like when it happens.

## Quality checks

```sh
yarn build-tsc   # type-check, .vue files included, via vue-tsc
yarn lint        # eslint over the repo, stylelint over .vue style blocks
yarn test        # vitest

cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Formatting is not a repo-wide command: the `.husky/pre-commit` hook runs `lint-staged`,
which applies prettier, eslint, stylelint, and `rustfmt` to staged files only.
