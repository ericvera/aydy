# 03_04 Delete the hook installer and port file, add the legacy cleanup

## Goal

The app no longer installs hooks or publishes a port. On every start it removes its old HTTP and curl hook entries from the user's Claude settings, writing only when some are present, so stale entries stop firing at a listener that is about to disappear.

## Files to modify/create

- src-tauri/src/hooks/merge.rs
- src-tauri/src/hooks/mod.rs
- src-tauri/src/hooks/legacy.rs (new)
- src-tauri/src/lib.rs
- src-tauri/src/listener/server.rs

## Files to delete

- src-tauri/src/hooks/install.rs
- src-tauri/src/hooks/stash.rs
- src-tauri/src/port_file.rs

## Background

- The extension now owns hook installation (stage 1). Task 03_03 removed the installer's IPC commands and `ListenerHealth`.
- What still uses the installer and port file is all in `lib.rs`:
  - `HookInstaller`, `HookStash`, `HookRegistry` (`:127-131`)
  - `follow_port_moves` (`:194`)
  - `PortFile`, `publish_port` (`:150`, `:199`)
  - the exit-time `PortFile::withdraw` (`:258`)
  - the constants `REMOVED_HOOKS_FILE` (`:53`) and `PORT_FILE`
- `hooks/merge.rs`:
  - `is_aydy_hook` (`:468-482`) identifies legacy entries: an http hook with header `X-Aydy-Hook`, or a command containing it.
  - `remove_aydy_hooks(hooks: &mut Map<String, Value>) -> usize` (`:171`).
  - It imports `listener::routes::HOOK_PATH` and `listener::server::RegisteredHooks` (`:10-11`).
  - Everything else in it (planned hooks, port rewrite, sound takeover, restore) serves the installer.
- `hooks/settings_file.rs` stays:
  - `SettingsFile::user()` (`:131`)
  - `read()` (`:144`)
  - `write(&snapshot, &value)` (`:186`), which is atomic and aborts if the file changed on disk
- `listener/server.rs`: `RegisteredHooks` and `HookRegistration` (`:261-276`) exist only for the installer.
- Spec decision (`.mise/spec.md`, "Hook entries"): the cleanup does **not** restore the `afplay` entries the old installer stashed in `removed-hooks.json`. The app keeps playing those sounds itself. Leave the stash file on disk.

## Guides

None match.

## Implementation details

- `merge.rs`:
  - Reduce it to `MARKER_HEADER`, `MARKER_VALUE`, `is_aydy_hook` and `remove_aydy_hooks`, plus the private helpers they need and their tests.
  - Delete the rest and its tests.
  - Drop the listener imports.
  - Rewrite the module doc: it recognises the entries older app versions installed.
- `legacy.rs`: `pub fn remove_legacy_hooks(settings: &SettingsFile) -> Result<usize, HookError>`.
  1. Read the file. A missing file returns 0.
  2. If `hooks` is absent, return 0.
  3. Clone the value and run `remove_aydy_hooks` on its `hooks` object.
  4. If the count is 0, return 0 **without writing**.
  5. Otherwise drop events that became empty, drop `hooks` if it became empty, and `write`.
- `hooks/mod.rs`: modules become `legacy`, `merge` and `settings_file`. Update the module doc. Keep `HookError` and `test_support`.
- `lib.rs`:
  - Remove the installer, stash, registry, port file and port publisher wiring and their constants.
  - At setup, spawn on the blocking pool: `SettingsFile::user().and_then(|f| remove_legacy_hooks(&f))`, logging errors with `eprintln!("aydy: ...")`.
  - The listener itself keeps running until 03_05.
- `server.rs`: delete `RegisteredHooks` and `HookRegistration` and anything that only they used.

## Gotchas

- This writes the user's `~/.claude/settings.json` (hard to undo). It must:
  - write nothing when there are no legacy entries
  - never touch entries containing `app.aydy/bin/aydy-hook.sh`, which belong to the extension
  - never touch `afplay` entries
  - abort when the file changed on disk, which `settings_file::write` already does
- `atomic_file.rs` loses `port_file` as a user. It is still used by `settings_file.rs`, `settings/store.rs` and `discovery/status_files.rs`.

## Verification

- New tests in `legacy.rs` using `hooks::test_support::settings_fixture` and `REAL_SETTINGS` (`hooks/mod.rs:579-680`), with legacy http and curl entries added. Check:
  - they are removed and the user's `afplay` hooks and the extension's `aydy-hook.sh` entries are untouched
  - other keys are byte-identical through `render`
  - a file with no legacy entries is not rewritten (mtime unchanged)
  - a missing file returns 0
  - malformed JSON returns an error and leaves the file unchanged
- Keep the `merge.rs` tests for `is_aydy_hook` and `remove_aydy_hooks`.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
