# 02_05 Jump by request and reply files (app side)

## Goal

Clicking a session row asks VS Code windows to reveal it by writing a request file, rather than calling one window over the loopback channel. The window that owns the session answers with a reply file, and the app raises that window. If no window answers, the popover says so.

## Files to modify/create

- src-tauri/src/jump/mod.rs
- src-tauri/src/lib.rs
- src-tauri/src/listener/server.rs
- src-tauri/src/listener/routes.rs
- src-tauri/src/ipc/commands.rs

## Background

- The jump today (`jump/mod.rs`):
  - `JumpOutcome` has `Jumped`, `Raised`, `NoWindow`, `Unconfirmed` and `Duplicated` (`:83-99`).
  - `JumpTarget{session_id, window, tab_label, cwd}` is built by `of()` (`:104-138`).
  - `WindowRaiser` (`:217`) is implemented by `CodeCli`, which runs `code <path>` (`:229-270`).
  - `IdeLocks` and `WindowFolders` (`:140-208`) and `JumpWatch` (`~:344-380`) support it.
  - `Jumper::jump` (`:491-560`) uses `ExtensionCommands` over loopback HTTP.
  - `JumpWatch::confirm` is called only from `listener/routes.rs:329`.
  - Wiring is in `lib.rs:136-149` and `:186-193`, plus `ListenerState::jumps` (`listener/server.rs:44-75`).
- The new protocol (`.mise/spec.md`, "Jump"), with files under `DataDir::jump_dir()` (task 02_02, `discovery/status_files.rs`):
  - Request `<id>.json`: `{"id","sessionId","cwd","tabLabel": string|null,"requestedAt": ms}`.
  - Reply `<id>.reply.json`: `{"open": "<abs path>", "reveal": "revealed"|"notPresent"|"unexpectedVersion"|"duplicated"|"failed"}`.
- The extension claims a request by unlinking it. Task 02_06 does the extension side.
- `jump_to_session` and `jump_target`: `ipc/commands.rs:199-238`.

## Guides

None match.

## Implementation details

- `JumpOutcome` becomes `Jumped | NoWindow | Unanswered | Duplicated`, serialized camelCase as today. Update the doc comments.
- Remove `window` from `JumpTarget`. Delete `IdeLocks`, `WindowFolders` and `JumpWatch`, and every use of `ExtensionCommands`.
- `Jumper::new(raiser: Arc<dyn WindowRaiser>, requests: JumpRequests)`, where `JumpRequests` is a small struct holding the dir, a timeout (3 s), a grace period (1 s) and a poll interval (50 ms). Tests use short values.
- `Jumper::jump(target)`:
  1. If `!raiser.available()` or `target.cwd` is None, return `NoWindow`.
  2. Write the request atomically under a unique id (pid plus a counter plus epoch ms is enough, so no uuid dependency).
  3. Poll for the reply until the timeout.
  4. On a reply, read it, delete it, and parse it.
     - `reveal == "duplicated"` returns `Duplicated`.
     - Otherwise `raise(open)`. Ok returns `Jumped`. Err returns `Unanswered`, because the window could not be brought forward.
  5. On timeout, `remove_file(request)`. `NotFound` means it was claimed late, so poll for the grace period, then return `Unanswered`. Any other result returns `Unanswered`.
  6. A malformed reply returns `Unanswered`.
- `lib.rs`:
  - Build `Jumper::new(Arc::new(CodeCli::default()), JumpRequests::new(data.jump_dir()))`.
  - Stop creating `JumpWatch` and stop passing it to `listener::server::run`.
  - Keep `ExtensionCommands` construction only if the listener still needs it. Otherwise remove it.
- `server.rs` and `routes.rs`: remove `jumps` from `ListenerState` and the `confirm` call at `routes.rs:329`.
- `commands.rs`: `jump_target` no longer reads `session.window`, and imports `lock_recovering` from `crate::sync`.

## Gotchas

- The reply's `open` path comes from another process. Pass it to `CodeCli::raise` as a path argument only, as today, and never through a shell.
- Delete the reply after reading it, and the request on timeout, so the jump dir does not grow.
- The frontend still knows `raised` and `unconfirmed`. Task 02_07 updates it, and the build stays green in between because the types are runtime strings.

## Verification

- Rewrite the `Jumper` tests in `jump/mod.rs` with a fake raiser and a scratch jump dir, simulating a responder thread:
  - a reply with `revealed` gives `Jumped` and a raise of `open`
  - `duplicated` gives `Duplicated` with no raise
  - no responder gives `Unanswered` and leaves no request file behind
  - a late claim (request unlinked, reply written within the grace period) gives `Jumped`
  - no `code` CLI gives `NoWindow` and writes nothing
  - a malformed reply gives `Unanswered`
- Keep the `JumpTarget::of` tests, minus `window`.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
