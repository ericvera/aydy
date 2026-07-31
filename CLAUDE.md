# AYDY

macOS menu bar app showing every active Claude Code session at a glance.

## Stack

- Tauri v2 (Rust core) — `src-tauri/`
- Vue `^3.5.40` + TypeScript + UnoCSS with a strict design preset — `src/`
- Dark mode only; English copy

## Workflow

Never open a pull request for, or merge into another branch, any branch whose tree contains `.mise/` — that work is still in flight; run `/mise:next` on that branch to finish acceptance and cleanup first.
