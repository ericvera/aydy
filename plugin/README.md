# AYDY plugin for Claude Code

Lets a Claude Code session report how much work is left in its turn to [AYDY](../README.md), the macOS menu bar app that shows every active Claude Code session. The estimate appears on the session's row in AYDY's popover and in the VS Code window title.

## Requirements

- macOS
- The AYDY app, the AYDY Companion VS Code extension, or both

Without AYDY on the machine (or on Linux) the command reports nothing and creates no files.

## Install

```
/plugin marketplace add ericvera/aydy
/plugin install aydy@aydy
```

## Usage

The plugin adds the `aydy:progress` skill. Claude uses it at the start of a multi-step task, after each step, and when its estimate changes, running:

```
aydy-progress --eta <duration> [--step <done>/<total>]
```

- `--eta` (required): time until the turn is done, such as `30s`, `6m`, `1h 10m` or `1h10m`, at most `24h`.
- `--step` (optional): steps done out of the total, such as `4/7`.

The session shows `Reported to AYDY: 4/7 · ~6m left`. The estimate covers the current turn only; a new prompt clears it.

## The file it writes

`~/Library/Application Support/app.aydy/progress/<session-id>.json`, for example:

```
{"etaSeconds":360,"step":{"done":4,"total":7}}
```

`step` is left out when not given, and the file's modification time is the report time. The session id comes from `CLAUDE_CODE_SESSION_ID`, which Claude Code sets for every Bash command.

## mise

To have mise tasks report their progress, add this line to the **Skills & guides** section of the repository's `.claude/mise-config.md`:

```
- aydy:progress (skill): at the start of each task and after each step
```
