# Review

## Changes

- Base (6485a74): single-folder windows auto-reopen as `<parent>/<folder>.code-workspace`; foreign file → overwrite prompt, decline → Application Support; legacy files migrate; AYDY removes its files for deleted folders; once-per-folder flag removed. Verify: open a folder with nothing running → window reloads onto `~/Code/<folder>.code-workspace`.
- 01_01 (85612be): auto-reopen blocks only on a working/waiting Claude Code session under this window's process, a terminal whose shell has a child process (or no pid), unsaved files, or a failed check. Verify: idle terminal → reopens silently; `sleep 300` in a terminal → notification naming it.
- Fix round 1 (98d40c3): README now names the `aydy.autoReopenAsWorkspace` setting.

## Open from review (minor, not fixed)

- Sibling path not writable (opening `~` or `/` as a folder) → write fails with EACCES and auto-reopen fails silently; no fallback to Application Support.
- Stray `console.info` on legacy migration (extension/src/extension.ts:306).

## Open assumptions

- A session with no status file yet counts as idle.
- A terminal is busy when its shell pid has any child process.
- A Notification event counts as waiting, even when it is Claude Code's idle reminder.
- The VS Code glue in `reasonsNotToReopen` has no unit test; checked by reading only.
- Not exercised in a real VS Code window yet.

## Amendments

- none
