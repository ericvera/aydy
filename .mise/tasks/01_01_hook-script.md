# 01_01 Hook script and shared paths

## Goal

Add the shell script that Claude Code hooks run. It writes one status file per session under AYDY's data directory. Also add the TypeScript module that names every shared path, so later tasks agree on where files live.

## Files to modify/create

- extension/hook/aydy-hook.sh (new)
- extension/src/aydyPaths.ts (new)

## Background

- Repo: AYDY, a macOS menu bar app (Tauri, `src-tauri/`) plus a companion VS Code extension (`extension/`). This work moves session tracking into files written by hooks, so tracking works with the app closed. The full design is in `.mise/spec.md` under "Shared files" and "Hook script".
- The data directory is `~/Library/Application Support/app.aydy/` (`extension/src/aydyClient.ts:194` and `:282` build the same base). Below it is `<data>`.
- The script ships in the VSIX, so it must live outside `extension/src/`: `extension/.vscodeignore:4` excludes `src/**`. `extension/hook/` is not excluded.
- Claude Code passes the hook payload as JSON on stdin. The fields used are `session_id`, `hook_event_name`, `cwd`, `transcript_path`, `message` and `notification_type` (`src-tauri/src/listener/routes.rs:41-56`).
- Pure-module pattern with a colocated test: `extension/src/revealSession.ts` / `revealSession.test.ts`. Tests run under vitest (`vitest.config.ts:19` includes `extension/src/**/*.test.ts`).
- Claude's config dir honours `CLAUDE_CONFIG_DIR` (`src-tauri/src/hooks/settings_file.rs:229-239`, `src-tauri/src/discovery/reconciler.rs:59-70`).

## Guides

None match.

## Implementation details

`extension/hook/aydy-hook.sh`, POSIX `sh`, header comment `# AYDY hook script v1`:

```sh
#!/bin/sh
dir="$HOME/Library/Application Support/app.aydy/status"
payload=$(cat)
field() { printf '%s\n' "$payload" | sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([A-Za-z0-9_-]\{1,128\}\)\".*/\1/p" | head -n 1; }
id=$(field session_id)
[ -n "$id" ] || exit 0
event=$(field hook_event_name)
if [ "$event" = SessionEnd ]; then rm -f "$dir/$id.json"; exit 0; fi
mkdir -p "$dir" 2>/dev/null || exit 0
tmp="$dir/.$id.$$.tmp"
printf '%s' "$payload" > "$tmp" 2>/dev/null && mv -f "$tmp" "$dir/$id.json" 2>/dev/null
rm -f "$tmp" 2>/dev/null
exit 0
```

- Every path must print nothing to stdout or stderr and exit 0.
- The file's mtime is the event time, and readers rely on it. Do not add a timestamp field.

`extension/src/aydyPaths.ts` exports pure functions. Each takes an optional `{home?: string, env?: NodeJS.ProcessEnv}` so tests never touch the real home:

- `aydyDataDir()` returns `<home>/Library/Application Support/app.aydy`.
- `statusDir()`, `seenDir()`, `jumpDir()`, `workspacesDir()`, `noticesDir()` and `binDir()` return the matching subdirectories.
- `hookScriptPath()` returns `<binDir>/aydy-hook.sh`.
- `claudeConfigDir()` returns `CLAUDE_CONFIG_DIR` when it is set and non-empty, otherwise `<home>/.claude`.
- `claudeSettingsPath()` returns `<config>/settings.json`.
- `claudeSessionsDir()` returns `<config>/sessions`.
- Constants: `HOOK_SCRIPT_MARKER = 'app.aydy/bin/aydy-hook.sh'` and `HOOK_SCRIPT_SOURCE = 'hook/aydy-hook.sh'`. The second is relative to the extension root.

## Gotchas

- macOS `sed` is BSD sed. Use only basic regular expressions with `\{1,128\}`, not `-E`, so the command stays portable.
- An embedded `"session_id"` inside a string value appears escaped (`\"session_id\"`) and must not match. Add a test for it.
- The data directory path contains a space. Quote everything.
- Do not import `vscode` in `aydyPaths.ts`. `uninstall.ts` runs in plain Node later.

## Verification

- New `extension/src/hookScript.test.ts` runs the script with `spawnSync('/bin/sh', [script], {input, env: {HOME: tmp, PATH: process.env.PATH}})` and covers:
  - A `Stop` payload writes `status/<id>.json` byte-identical to the input.
  - `SessionEnd` removes it.
  - A payload with no `session_id`, invalid JSON, and an id containing `/` or `..` write nothing and exit 0.
  - A `message` field containing `\"session_id\":\"evil\"` still writes under the real id.
  - stdout and stderr are empty in every case.
- New `extension/src/aydyPaths.test.ts` covers the default paths and the `CLAUDE_CONFIG_DIR` override, including an empty override.
- Run `yarn test`, `yarn build-tsc` and `yarn lint`.
