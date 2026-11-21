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
