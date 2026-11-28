# 03_01 Plugin with the progress skill and command

## Goal

Ship a Claude Code plugin from this repository's own marketplace: plugin `aydy` with skill `progress` (`aydy:progress`) and its command `aydy-progress --eta <duration> [--step N/M]`, which writes the session's progress file that AYDY reads.

## Files to modify/create

- `.claude-plugin/marketplace.json` (new, repository root)
- `plugin/.claude-plugin/plugin.json` (new)
- `plugin/skills/progress/SKILL.md` (new)
- `plugin/skills/progress/scripts/aydy-progress` (new, executable POSIX sh)
- `plugin/README.md` (new)
- test: `extension/src/progressScript.test.ts` (new; tests do not count toward the limit)

## Background

AYDY is a macOS menu bar app (this repo) that lists every Claude Code session; its VS Code extension and Rust app read per-session files under `~/Library/Application Support/app.aydy/`. The existing hook script `extension/hook/aydy-hook.sh` is the style model: POSIX sh, `set`-free, atomic write via a temp file in the target dir then `mv -f`, never leaves temp files, validates the session id with a strict pattern.

Owner decisions (`.mise/goals.md` decisions 2, 6, 7, 9):

- The skill ships as a Claude Code plugin from this repo's own marketplace, installed with `/plugin install`; AYDY never writes the skill into `~/.claude`.
- Only `--eta` (required) and optional `--step`. **No `--doing`.**
- mise integration is documentation only: the README shows the line `- aydy:progress (skill): at the start of each task and after each step` for a repo's `.claude/mise-config.md`.
- Use the product name for every identifier (marketplace `aydy`, plugin `aydy`) — never anything derived from the owner's identity.

Progress file contract (spec `## Design`; the Rust app and the extension read it):

- Path `$HOME/Library/Application Support/app.aydy/progress/<session-id>.json`; the file mtime is the report time.
- Body exactly `{"etaSeconds":360,"step":{"done":4,"total":7}}`, `step` omitted when not given.
- Session id from `$CLAUDE_CODE_SESSION_ID` (Claude Code sets it in every Bash tool call; subagents inherit the parent's), valid only if it matches `^[A-Za-z0-9_-]{1,128}$`.

Model plugin layouts on this machine: `~/.claude/plugins/marketplaces/delta-review/.claude-plugin/marketplace.json` (plugin `source: "./plugin"`) and `.../delta-review/plugin/.claude-plugin/plugin.json`; skill frontmatter and `${CLAUDE_SKILL_DIR}` usage in `~/.claude/plugins/marketplaces/ericvera/skills/next/SKILL.md` (`allowed-tools: Bash(node ${CLAUDE_SKILL_DIR}/scripts/state.ts …)`).

The approved mock (not part of the repository) showed this SKILL.md tone, adapted here to the decisions (no `--doing`; the skill comes from the plugin, AYDY does not install it):

```
Run once you know the shape of the task, and again when it changes:
  ${CLAUDE_SKILL_DIR}/scripts/aydy-progress --eta 6m --step 4/7
  --eta   how long until this turn is done: 30s, 6m, 1h 10m. Your best guess.
  --step  optional. Steps done / total, if you are following a plan.
The estimate covers this turn only. A new prompt clears it.
Say when you are blocked by asking the user, not with --eta.
```

and the session output `Reported to AYDY: 4/7 · ~6m left`, then later `Reported to AYDY: 6/7 · ~2m left` for `--eta 2m --step 6/7`.

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `.claude-plugin/marketplace.json`: `{"name":"aydy","owner":{"name":"AYDY"},"description":"…","plugins":[{"name":"aydy","source":"./plugin","description":"Lets Claude Code sessions report how much work is left to the AYDY menu bar app."}]}`.
2. `plugin/.claude-plugin/plugin.json`: `name: "aydy"`, description, `version: "0.1.0"`.
3. `plugin/skills/progress/SKILL.md` frontmatter: `name: progress`; a `description` that makes the model use it at the start of any multi-step task, after each step, and whenever the estimate changes by more than a couple of minutes; `allowed-tools: Bash(${CLAUDE_SKILL_DIR}/scripts/aydy-progress:*)`. Body: what it is for (the owner sees it in AYDY's menu bar and VS Code window title), the exact command `${CLAUDE_SKILL_DIR}/scripts/aydy-progress --eta 6m --step 4/7` written **unquoted**, so the Bash command starts with the same text as the `allowed-tools` prefix rule and runs without a permission prompt (the installed mise skill does the same: `node ${CLAUDE_SKILL_DIR}/scripts/state.ts …` unquoted in both places), the flags, that the ETA covers the current turn only and a new prompt clears it, that a wait on the user is shown by asking, not by `--eta`, and that a failure to report must never stop the task (report the error once and continue).
4. `aydy-progress` (`#!/bin/sh`, mode 755, header comment `# AYDY progress command v1`):
   - Parse `--eta <value>` and `--step <value>` (also `--eta=<value>` / `--step=<value>`); `-h`/`--help` prints usage to stdout and exits 0. Unknown flag, missing or repeated `--eta` → usage on stderr, exit 2.
   - ETA grammar: one to three tokens in order `<n>h`, `<n>m`, `<n>s`, optionally separated by single spaces (`30s`, `6m`, `1h 10m`, `1h10m`), integers only; total 0 to 86400 seconds; anything else → error naming the accepted forms, exit 2.
   - Step: `<done>/<total>` integers, `total >= 1`, `done <= total`; else exit 2.
   - Session id: `$CLAUDE_CODE_SESSION_ID` checked against the pattern (use `case`/`expr`, not bash-isms); missing or invalid → `aydy-progress: not running inside a Claude Code session; nothing reported` on stderr, exit 1.
   - AYDY not installed: when `"$HOME/Library/Application Support/app.aydy"` is not a directory (no AYDY app or extension on this machine, or Linux), create nothing and exit 0 with one line on stdout: `AYDY is not installed here; nothing reported`. This check comes after argument and session-id validation, so usage errors still surface. Never `mkdir -p` the `app.aydy` directory itself.
   - Write: `dir="$HOME/Library/Application Support/app.aydy/progress"`, `mkdir` it (its parent exists by now), write JSON to `"$dir/.progress.$$.tmp"`, `mv -f` to `"$dir/$id.json"`, remove the temp file on any failure; write failure → message on stderr, exit 1.
   - Success: stdout `Reported to AYDY: 4/7 · ~6m left` (step part only when given; durations formatted like the popover — `1h 10m`, `6m`, `45s`, rounded down).
5. `plugin/README.md`: what it does; requirements (macOS, AYDY app and/or the AYDY Companion VS Code extension); install `/plugin marketplace add ericvera/aydy` then `/plugin install aydy@aydy`; usage; the file it writes; a **mise** section with the exact Skills & guides line to paste into `.claude/mise-config.md`: `- aydy:progress (skill): at the start of each task and after each step`.

## Gotchas

- POSIX sh only (macOS `/bin/sh`): no arrays, `[[`, `$((…))` is fine; test with `/bin/sh`.
- `git add` must keep the executable bit (`git update-index --chmod=+x` if needed).
- Prettier formats staged `*.json`/`*.md` via lint-staged; let it.
- `${CLAUDE_SKILL_DIR}` is substituted by Claude Code in SKILL.md content and `allowed-tools`; do not hard-code paths.
- Do not touch `~/.claude` or install the plugin as part of this task.
- The plugin is public: a user without AYDY, or on Linux, must not get a stray `~/Library/Application Support/app.aydy` tree from it.
- The unquoted invocation relies on the plugin's install path having no spaces (Claude Code's plugin cache under `~/.claude/plugins/` does not); do not add quotes to work around a hypothetical path.

## Verification

- Run `yarn test` (and `yarn lint`, `yarn build-tsc`).
- Write `extension/src/progressScript.test.ts` (vitest already includes `extension/src/**/*.test.ts`; follow `extension/src/hookScript.test.ts`: temp `HOME` with `Library/Application Support/app.aydy` created for the write cases, `spawnSync('/bin/sh', [SCRIPT, ...args], { env })`, `SCRIPT = join(__dirname, '..', '..', 'plugin', 'skills', 'progress', 'scripts', 'aydy-progress')`):
  - `--eta 6m --step 4/7` writes `{"etaSeconds":360,"step":{"done":4,"total":7}}` to `progress/<id>.json`, prints `Reported to AYDY: 4/7 · ~6m left`, exits 0, leaves no temp file;
  - `--eta "1h 10m"` and `--eta 1h10m` → 4200; `--eta 45s` prints `~45s left`; no `--step` omits `step`;
  - a second report replaces the first;
  - invalid eta (`6`, `6x`, `-1m`, `25h`), invalid step (`5/4`, `1/0`, `a/b`), unknown flag, missing `--eta` → exit 2, nothing written;
  - no or invalid `CLAUDE_CODE_SESSION_ID` (`../x`) → exit 1, nothing written;
  - temp `HOME` without `app.aydy` and a valid session id → exit 0, stdout `AYDY is not installed here; nothing reported`, and `HOME/Library` still does not exist;
  - the script file is executable.
- Substitute check: `SKILL.md`'s command line and its `allowed-tools` rule both start with the unquoted `${CLAUDE_SKILL_DIR}/scripts/aydy-progress`; `jq . .claude-plugin/marketplace.json plugin/.claude-plugin/plugin.json` parses; if the `claude` CLI is available, `claude plugin validate .` (skip silently if the subcommand does not exist).
