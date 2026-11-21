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
- The request's `cwd` is the session's **launch** directory (`.mise/spec.md`, "Session directory"). `JumpTarget::of` takes `session.cwd`, which the registry overwrites from every hook event (`session/registry.rs:208-209`) and so follows the session when it changes directory, for example from window `/a` into `/a/.claude/worktrees/x`. The extension would then let the worktree window claim a session whose tab lives in the `/a` window. The launch directory is the `cwd` field of the session's live `~/.claude/sessions/<pid>.json` (`REAL_SESSION_FILE`, `discovery/mod.rs:48-60`), read with `discovery::session_files::read_all` (`session_files.rs:71`), with liveness from `ProcessTable` (`session_files.rs:139`; `LiveProcesses` in production, `FixedProcesses` in tests). The sessions dir is `discovery::reconciler::user_sessions_dir()` (`reconciler.rs:59`). Test helpers: `discovery::test_support::{sessions_dir, REAL_SESSION_FILE}` (`discovery/mod.rs`).
- `jump_to_session` and `jump_target`: `ipc/commands.rs:199-238`.
- Imports in `jump/mod.rs` that must all go, because `listener/` and `extension/` are deleted in 03_05 and `WindowFocus` in 03_07:
  - `:32` `crate::extension::command::{ExtensionCommands, RevealOutcome}`
  - `:33` `crate::extension::registry::{ExtensionPid, Registration}`
  - `:34` `crate::listener::server::lock_recovering` (switch to `crate::sync::lock_recovering`, in the tests too, where the fakes at `:731-827` call it)
  - `:35` `crate::session::event::WindowId` and the `WindowFocus` part of `:37`
  - the test module's `crate::extension::command::ExtensionChannel` and `crate::extension::registry::ExtensionRegistry` (`:639-640`)
- `jump/mod.rs:1444`: the test `a_session_that_cannot_be_jumped_to_has_nothing_to_jump_to` sets `session.window = Some(65_921);`. `Session.window` is deleted in 03_06, whose files do not include `jump/mod.rs`, so this task removes the line.
- `ipc/commands.rs` test module (`:458-480`) builds its `jumpable_session` fixture (`:801-831`) by placing the session in a window: it imports `crate::discovery::process_tree::FixedParents` (`:465`), `crate::discovery::window_map::associate` (`:467`) and `crate::extension::registry::{ExtensionPid, Registration}` (`:468`), defines `const WINDOW: ExtensionPid` (`:798`), and calls `registry.apply_window_map(&associate(...))` (`:819-828`). `apply_window_map`, `window_map.rs` and `process_tree.rs` are deleted in 03_06, and `extension/` in 03_05, so this fixture must stop using them here.
- `Registration` is **not** only the fixture's: the helper `fn window(pid: u32) -> Registration` (`ipc/commands.rs:954`) and the `extension_status` tests after it still use it. They are deleted in 03_03, which also drops the import. This task keeps it.

## Guides

None match.

## Implementation details

- `JumpOutcome` becomes `Jumped | NoWindow | Unanswered | Duplicated`, serialized camelCase as today. Update the doc comments.
- Remove `window` from `JumpTarget`. Delete `IdeLocks`, `WindowFolders` and `JumpWatch`, and every use of `ExtensionCommands`.
- Add `pub fn with_launch_cwd(self, sessions_dir: Option<&Path>, processes: &dyn ProcessTable) -> Self` to `JumpTarget`. When `sessions_dir` is `Some` and `session_files::read_all` succeeds, it looks for a file whose `session_id` equals the target's, whose `pid` `processes.is_alive`, and whose `cwd` is `Some`, and replaces `self.cwd` with that `cwd`. In every other case (no dir, unreadable dir, no matching live file, a file without `cwd`) it returns the target unchanged, so the registry's `session.cwd` is the fallback. Doc it with the reason above.
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
- `commands.rs`:
  - `jump_target` no longer reads `session.window`. (Task 02_03 already switched `lock_recovering` to `crate::sync`.)
  - `jump_to_session`: inside the `spawn_blocking` closure, before `jumper.jump`, set `let target = target.with_launch_cwd(user_sessions_dir().as_deref(), &LiveProcesses);`. It reads files, so it runs on the blocking thread and never while the registry lock is held. The `unwrap_or` fallback becomes `JumpOutcome::Unanswered`, since `Unconfirmed` is gone.
  - In the test module, remove the window placement from `jumpable_session`: delete the `apply_window_map(&associate(...))` call, `const WINDOW`, and the `FixedParents`, `associate` and `ExtensionPid` imports (keep `SessionFile` only if something else uses it). Narrow `:468` to `use crate::extension::registry::Registration;` and keep it: the `window` helper (`:954`) and the `extension_status` tests still need it until 03_03. Update the fixture's doc comment: the session is found by the scan and titled by the transcript pass, and no window placement is needed for a jump any more.

## Gotchas

- Parse the reply's `reveal` as a plain string and compare it with `"duplicated"`. Do not import `RevealOutcome` from `crate::extension`.
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
- `with_launch_cwd` tests in `jump/mod.rs`, using a scratch sessions dir from `discovery::test_support::sessions_dir` and `FixedProcesses`:
  - a target whose `cwd` is `/a/.claude/worktrees/x`, with a live session file for its id whose `cwd` is `/a`, comes out with `cwd` `/a`
  - a matching file whose pid is dead, a file for another session id, a file without `cwd`, a `None` dir and a missing dir all leave `cwd` unchanged
- Keep the `JumpTarget::of` tests, minus `window`. In `a_session_that_cannot_be_jumped_to_has_nothing_to_jump_to`, delete the `session.window = Some(65_921);` line and keep the test: `jumpable = false` alone gives `None`.
- The `commands.rs` jump tests that use `jumpable_session` still pass without the window placement.
- Grep `jump/mod.rs` and the `commands.rs` test module for `extension::`, `listener::`, `ExtensionPid`, `WindowId`, `WindowFocus`, `FixedParents`, `window_map`, `apply_window_map`, and the field access `.window` (`grep -nE '\.window\b'`, so `.windows` and `.window_count` do not count). None may remain, except the one line `use crate::extension::registry::Registration;` in the `commands.rs` test module, which 03_03 removes.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
