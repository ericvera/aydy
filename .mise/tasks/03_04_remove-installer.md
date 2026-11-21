# 03_04 Delete the hook installer and port file, add the legacy cleanup

## Goal

The app no longer installs hooks or publishes a port. On every start it removes its old HTTP and curl hook entries from the user's Claude settings, writing only when some are present, so stale entries stop firing at a listener that is about to disappear. It also deletes the old installer's stash file `removed-hooks.json` without restoring the `afplay` hooks stashed in it.

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

- The extension now owns hook installation (stage 1). Task 03_03 removed the installer's IPC commands and stopped creating `ListenerHealth`, but left the type in `listener/server.rs` (`:278-300`) because the `hooks/install.rs` tests (`:511`, `:1077-1112`) construct it. This task deletes both.
- What still uses the installer and port file is all in `lib.rs`:
  - `HookInstaller`, `HookStash`, `HookRegistry` (`:127-131`)
  - `follow_port_moves` (`:194`)
  - `PortFile`, `publish_port` (`:150`, `:199`)
  - the exit-time `PortFile::withdraw` (`:258`)
  - the constants `REMOVED_HOOKS_FILE` (`:53`, `"removed-hooks.json"`, joined onto `app.path().app_config_dir()` at `:129`) and `PORT_FILE`
- `hooks/merge.rs`:
  - `is_aydy_hook` (`:468-482`) identifies legacy entries: an http hook with header `X-Aydy-Hook`, or a command containing it.
  - `remove_aydy_hooks(hooks: &mut Map<String, Value>) -> usize` (`:171`).
  - It imports `crate::hooks::stash::RemovedEntry` (`:9`), used only by the installer's sound takeover and restore, and `listener::routes::HOOK_PATH` and `listener::server::RegisteredHooks` (`:10-11`).
  - Everything else in it (planned hooks, port rewrite, sound takeover, restore) serves the installer.
- `hooks/settings_file.rs` stays:
  - `SettingsFile::user()` (`:131`)
  - `read()` (`:144`)
  - `write(&snapshot, &value)` (`:186`), which is atomic and aborts if the file changed on disk
  - `SettingsFile::new(path)` (`:125`) and `path()` (`:136`)
- Symlinks: `write` goes through the private `SettingsFile::write_atomically` (`settings_file.rs:220-224`), which calls `atomic_file::write_atomically(&self.path, ..)`. That writes a temp file next to the path it is given and renames onto that path (`atomic_file.rs:60`). Given a symlinked `~/.claude/settings.json` (dotfiles managers), the rename replaces the link with a regular file and the dotfiles copy silently stops being used. The extension's writer already resolves the link first (01_02); this cleanup must too.
- `listener/server.rs`: `RegisteredHooks` and `HookRegistration` (`:261-276`) exist only for the installer, and `ListenerHealth` (`:278-300`, with its test helper `health` at `:975-982`) only for the health snapshot 03_03 removed.
- Owner decision 4 (`.mise/goals.md`, Decisions): the `afplay` sound hooks the old installer stashed are not restored. Sounds belong to the app, and restoring them would double every sound. Stage 3's legacy cleanup deletes the stash file `removed-hooks.json` along with the old `X-Aydy-Hook` entries.

## Guides

None match.

## Implementation details

- `merge.rs`:
  - Reduce it to `MARKER_HEADER`, `MARKER_VALUE`, `is_aydy_hook` and `remove_aydy_hooks`, plus the private helpers they need and their tests.
  - Delete the rest and its tests.
  - Drop the `use crate::hooks::stash::RemovedEntry;` import (`:9`), which no longer resolves once `stash.rs` is deleted, and the listener imports (`:10-11`).
  - Rewrite the module doc: it recognises the entries older app versions installed.
- `legacy.rs`: `fn resolved(settings: &SettingsFile) -> Option<SettingsFile>`. When `fs::symlink_metadata(settings.path())` says the path is a symlink, return `SettingsFile::new(fs::canonicalize(path)?)` (the real file, following chains of links); `None` when canonicalize fails. That is a dangling link, which the spec ("Shared files") treats as its missing target in both settings writers: the extension's install creates the target, and this cleanup treats it as a missing file, so there is nothing to clean and it creates nothing. Otherwise return a `SettingsFile` for the same path. This keeps the fix inside `legacy.rs`, so `settings_file.rs` is not touched.
- `legacy.rs`: `pub fn remove_legacy_hooks(settings: &SettingsFile) -> Result<usize, HookError>`.
  0. Resolve the target with `resolved`; `None` returns 0. Every step below reads and writes the resolved file, so the rename lands on the real path and the link survives. `atomic_file::write_atomically` carries the real file's permissions over (`atomic_file.rs:57-59`).
  1. Read the file. A missing file returns 0.
  2. If `hooks` is absent, return 0.
  3. Clone the value and run `remove_aydy_hooks` on its `hooks` object.
  4. If the count is 0, return 0 **without writing**.
  5. Otherwise drop events that became empty, drop `hooks` if it became empty, and `write`.
- `legacy.rs`: `pub const LEGACY_STASH_FILE: &str = "removed-hooks.json"` and `pub fn remove_legacy_stash(path: &Path) -> io::Result<bool>`, which runs `fs::remove_file` and returns `Ok(true)` when it deleted the file and `Ok(false)` on `NotFound`. It never reads the file and never restores anything from it.
- `hooks/mod.rs`: modules become `legacy`, `merge` and `settings_file`. Update the module doc. Keep `HookError` and `test_support`.
- `lib.rs`:
  - Remove the installer, stash, registry, port file and port publisher wiring and their constants.
  - At setup, spawn on the blocking pool: `SettingsFile::user().and_then(|f| remove_legacy_hooks(&f))`, then `remove_legacy_stash(&config.join(LEGACY_STASH_FILE))` regardless of the first result, logging each error with `eprintln!("aydy: ...")`. `config` is `app.path().app_config_dir()?`, the directory the stash lived in.
  - The listener itself keeps running until 03_05.
- `server.rs`: delete `RegisteredHooks`, `HookRegistration` and `ListenerHealth`, their tests and test helpers, and anything that only they used.

## Gotchas

- This writes the user's `~/.claude/settings.json` (hard to undo). It must:
  - write nothing when there are no legacy entries
  - never touch entries containing `app.aydy/bin/aydy-hook.sh`, which belong to the extension
  - never touch `afplay` entries
  - abort when the file changed on disk, which `settings_file::write` already does
  - leave a symlinked settings file a symlink, by writing to its resolved real path
- Deleting `removed-hooks.json` is one-way: the stashed `afplay` entries cannot be restored afterwards. That is the owner's decision. Do not read, log or restore its contents.
- `atomic_file.rs` loses `port_file` as a user. It is still used by `settings_file.rs`, `settings/store.rs` and `discovery/status_files.rs`.

## Verification

- New tests in `legacy.rs` using `hooks::test_support::settings_fixture` (`hooks/mod.rs:190`) and `REAL_SETTINGS` (`hooks/mod.rs:94`), with legacy http and curl entries added. Check:
  - they are removed and the user's `afplay` hooks and the extension's `aydy-hook.sh` entries are untouched
  - other keys are byte-identical through `render`
  - a file with no legacy entries is not rewritten (mtime unchanged)
  - a missing file returns 0
  - malformed JSON returns an error and leaves the file unchanged
  - symlink: in a scratch dir, a real `dotfiles/settings.json` (mode 0600, with legacy entries) and a symlink `claude/settings.json` pointing at it; after `remove_legacy_hooks(&SettingsFile::new(<link>))`, `fs::symlink_metadata(<link>)` is still a symlink to the same target, the real file has the legacy entries removed and is still mode 0600, and no temp file is left in either directory
  - a dangling symlink returns 0 and creates nothing
  - `remove_legacy_stash` deletes a scratch `removed-hooks.json` and returns true, returns false when it is absent, and the settings file is not touched by it
- Keep the `merge.rs` tests for `is_aydy_hook` and `remove_aydy_hooks`.
- Grep `src-tauri/src` for `ListenerHealth`, `HookInstaller`, `HookStash`, `RegisteredHooks` and `port_file`; none may remain.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
- Do not launch the app (`yarn tauri dev` or any build) to watch the cleanup run: it would rewrite the owner's real `~/.claude/settings.json`, whose `X-Aydy-Hook` entries the installed app still depends on, and delete the real `removed-hooks.json`. Every test uses scratch paths through `SettingsFile::new`, never `SettingsFile::user()`. The real run is the owner's at acceptance (spec, "Verification boundary").
