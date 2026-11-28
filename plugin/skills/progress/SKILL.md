---
name: progress
description: |
  Reports how much work is left in this turn to the AYDY menu bar app. Use at
  the start of any multi-step task, after each step, and whenever your
  estimate changes by more than a couple of minutes.
allowed-tools: Bash(${CLAUDE_SKILL_DIR}/scripts/aydy-progress:*)
---

# Progress

Tells the owner how long this turn has left. They see it in AYDY's menu bar and in the VS Code window title, so they know when to come back without watching the session.

## Report

Run once you know the shape of the task, and again after each step or when the estimate changes:

```
${CLAUDE_SKILL_DIR}/scripts/aydy-progress --eta 6m --step 4/7
```

- `--eta` (required): how long until this turn is done: `30s`, `6m`, `1h 10m`. Your best guess.
- `--step` (optional): steps done / total, if you are following a plan.

The estimate covers this turn only. A new prompt clears it.

Say when you are blocked by asking the user, not with `--eta`.

## Failures

A failed report must never stop the task. If the command fails, mention the error once and carry on without reporting again.
