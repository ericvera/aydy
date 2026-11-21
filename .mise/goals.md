make the change and validate the changes here using mise.

## Context from the conversation

"The change" is the one proposed just before this request, on top of the uncommitted
auto-reopen work already on this branch (extension/src/autoReopen.ts, processTree.ts,
workspaceFile.ts, extension.ts, extension/README.md, extension/package.json):

Tighten the "is it safe to auto-reopen this window as its workspace" check so it blocks
only on things that are actually running and would be lost:

- a Claude Code session in this window that is working or waiting on the user (idle
  sessions no longer block — a restarted session always comes back idle anyway);
- a terminal whose shell has a child process running (a dev server, a build, a Claude
  Code CLI) — an idle shell at a prompt no longer blocks;
- unsaved files (unchanged).

"Validate the changes here" means the whole branch's change — the existing auto-reopen
work plus this refinement — goes through mise's review.

Owner decisions already made in the conversation:

- Auto-reopen is on by default (`aydy.autoReopenAsWorkspace`), silent when safe, and
  shows a notification with "Reopen anyway" when not safe.
- The once-per-folder offer flag is gone.
- Workspace files live beside the folder (`<parent>/<folder>.code-workspace`); a
  foreign file there prompts to overwrite, declining falls back to Application Support.
- AYDY removes its own workspace files whose folder no longer exists.

## Investigation

- Safety check: `unsafeReasons`/`WindowContents` in extension/src/autoReopen.ts:11-31; inputs built in `reasonsNotToReopen`, extension/src/extension.ts:189-203 (terminal count, `isClaudeTab` tab count, `sessionsUnder(process.pid)`, failure counted as unsafe). Legacy migration also uses it (extension.ts:264).
- Session state: only the last hook event per session is kept (`SessionStatus.event`, extension/src/sessionState.ts:1-9). UserPromptSubmit = working, Notification = waiting, Stop/SessionStart = idle; Rust uses the same mapping (src-tauri/src/session/session.rs:117-132).
- Matching sessions to the window: `sessionsUnder` (processTree.ts:91-98) counts by pid descent but `readSessionPids` drops `sessionId`, needed to join status files (`readStatuses`, sessionStore.ts:60). The alternative is WindowTracker's folder-based `ownerOf` (windowTracker.ts:189-205, sessionState.ts:97).
- Claude tabs carry no session state, so they cannot tell idle from working.
- Terminal child processes: `parseParents` (processTree.ts:8-46) gives child→parent; a shell is busy when some ppid equals `Terminal.processId`. Pseudoterminals have no processId; a restored terminal's pid may not be available yet.
- Ambiguity: Claude Code also fires Notification for its idle "waiting for your input" reminder, so an idle session can read as waiting.
- Copy to update: README "Automatically." bullet and the `aydy.autoReopenAsWorkspace` markdownDescription (package.json:32).
- Tests: `unsafeReasons` cases in autoReopen.test.ts:39-60; a pure "shell has children" helper fits beside `descendsFrom` tests in processTree.test.ts.
- Hard to undo (from the branch's existing work): `sweepWorkspaces` and `removeLegacyWorkspace` delete files; `forgetOfferFlags` clears globalState; the public setting defaults to true; a false "safe" restarts the window and ends live work.
- No UI mock needed (no Vue change).

## Open questions

1. Match sessions to this window by process ancestry or by folder?
2. Do Claude Code tabs still block on their own?
3. When state is unknown (terminal with no pid, Notification that may be the idle reminder, check failure), block or not?
