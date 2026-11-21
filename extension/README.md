# AYDY Companion

Tells **AYDY** which Claude Code sessions belong to this VS Code window.

AYDY lists every active Claude Code session in the macOS menu bar. Without this
extension it still lists and groups them, and clicking a session still raises a VS Code
window where the folder identifies one. What this adds is the window itself: which
sessions are in which window, which tab is in front, and the ability to be sent to one.

## Installing it

Package it first — see [Building it](#building-it) — and then hand the artifact to VS
Code, from the repository root:

```sh
code --install-extension extension/aydy-companion-0.1.0.vsix
```

Uninstall the same way, so that the record of the install is unwound as well as the
files:

```sh
code --uninstall-extension aydy.aydy-companion
```

**Never copy the folder into `~/.vscode/extensions` by hand.** VS Code keeps its own list
of what is installed in `~/.vscode/extensions/extensions.json` and only the installer
writes it. A folder placed beside that list rather than through it leaves the two free to
disagree, and the way they disagree is loud: an entry naming a folder that is not there
is scanned as an extension whose manifest cannot be read, which is reported as **Invalid
extensions detected** in the Extensions view of every open window until the entry is
removed.

Beyond installing there is nothing to configure, and this extension contributes no
settings on purpose. AYDY publishes the port it is listening on and the extension reads
it; if AYDY is not running, or is not installed, the extension does nothing at all and
says nothing about it.

## What it reports

Once per window, then on a heartbeat, and again the moment one of them changes:

- **The extension host's process id.** VS Code has no window identifier of any kind, but
  each window runs its own extension host and every Claude Code session started in a
  window is a descendant of that window's host. So this number is what lets AYDY say
  which sessions belong here.
- **The window's workspace folders**, which is how AYDY knows a window holding a given
  repository exists before it tries to raise one.
- **The version of the Claude Code extension**, if it is installed here.

Folders are added to and removed from a multi-root workspace without VS Code restarting
the extension host, so a change to them is reported when it happens rather than at the
next reload — AYDY chooses the window to send a session to from that list, and a stale
one sends it to the wrong window.

And whenever they change, debounced so that alt-tabbing does not cost a report per flap:

- **Whether this window has focus.**
- **The label on this window's active Claude Code tab**, or nothing when the tab in
  front is not one. A session opened in the sidebar is a view rather than a tab and
  never appears here at all.

Those are two facts and they are sent as two. A Claude tab left in front of a window
sitting behind a browser means the user is **not** watching that session, which is
precisely the case AYDY exists to notify about — so a single "being watched" flag would
silently switch notifications off for anyone who leaves VS Code open in the background.
Losing focus is reported as carefully as gaining it, for the same reason: a window AYDY
still believes is in front goes on silencing its own sessions.

Both are also said again whenever AYDY takes a fresh registration of this window. An AYDY
that has just started holds nothing about where the user is, and these two are the only
things reported on change alone — so an editor sitting untouched while AYDY restarts
would otherwise go unaccounted for until the user next moved focus or switched tabs.

It reports facts and runs commands. It does not decide which session is which, match
titles, or hold any of AYDY's state — AYDY is the only side that can see every window at
once. The tab label goes over raw and AYDY matches it against the session titles it
holds, cut the way the editor cuts them; when two sessions in this window answer to the
same label, or nothing AYDY holds answers to it at all, AYDY treats the tab as
unidentified rather than guessing. An unread the user has to click is recoverable, and an
unread cleared for a session they never looked at is not.

A tab goes unidentified more often than it looks like it should, and always in that safe
direction. The editor labels a session's tab with its first prompt from the moment that
prompt is sent, and only replaces it once a title has been generated; the placeholder
`Claude Code` is what a session shows before it has been prompted at all. AYDY holds the
generated titles and nothing else, so any tab still showing a prompt or the placeholder
matches nothing. A session **renamed by hand** never matches either: a rename is recorded
as a custom title, and AYDY does not read those. Those sessions clear their unread the
way they always have — by being clicked.

## Revealing a tab

The one thing AYDY asks this window to **do**: bring the tab that owns a session to the
front, so that a window with three sessions in it comes up showing the right one.

There is no supported way to do it. `vscode.TabGroups` can close a tab and nothing else,
and the Claude Code extension exports no API, so the only way in is the command it
contributes — `claude-vscode.editor.open`, called the way Claude's own webview calls it.
Given a session this window already has open, that command reveals the existing panel.
**Given one it does not, the same call creates a panel and resumes the session in it** —
a second, live Claude session running against the user's work, started silently.

So a reveal is refused unless every one of these holds, and refusing is reported to AYDY
rather than swallowed:

- **AYDY placed the session in this window**, through the process tree. It never sends a
  reveal for a session it could not place.
- **The command names this window's process id.** A window that closes frees the port it
  named and the next window to open can be given it, so a reveal arriving here for
  another process is a reveal for a session this window does not have.
- **A tab here carries the label AYDY expects**, which is the session's title cut the way
  the editor cuts it. A session hosted in the sidebar is a view rather than a tab and
  never matches — correctly, since it has no tab to reveal.
- **Claude Code is on the release line the call was read out of.** The argument order is
  undocumented and the bundle ships roughly weekly, so anything else degrades to raising
  the window without the tab.

Afterwards, a Claude tab that **opened** while the call was made is reported as a
duplicate rather than as a successful reveal. It means a guard was wrong, and a second
copy of a session the user does not know about is worse than one they are told about.

## The session count in the tab title

A window can show how many of its Claude Code sessions need you in its own title: `○
okven` when none do, `● okven` for one, `● 3 okven` for several and `● 9+ okven` past
nine. The count is this window's sessions that finished or asked for something since you
last had the window focused.

VS Code only lets an extension set a window's title through a settings file, and a
plain-folder window's only settings file is the repository's own `.vscode/settings.json`.
So the title is written into a workspace file AYDY generates and keeps **outside the
repository**, at
`~/Library/Application Support/app.aydy/workspaces/<hash>/<folder>.code-workspace`, one
per folder. The folder's own `.vscode/settings.json` is copied into that file, and copied
again whenever it is created, changed or deleted, so the window keeps behaving the way
the repository says. Whether a window-scoped setting in `.vscode/settings.json` applies
in such a window only through that copy has **not been verified**.

Getting a window onto that file means reopening it:

- **The offer.** The first time a folder is opened as a plain single-folder window, a
  notification asks whether to reopen it as a workspace, with **Not now** and **Reopen as
  workspace**. It is shown once per folder, whichever you pick.
- **The command.** `AYDY: Show Session Count in Tab Title` in the command palette does
  the same reopen at any time. It is listed only in a plain-folder window.

Nothing reopens without that click. **Reopening restarts the window**, which may end
Claude Code sessions open in it.

Plain-folder windows keep VS Code's default title, and a window opened from a
`.code-workspace` of your own is left alone: nothing is written there. Folder names go
into the title literally, but VS Code still expands `${...}` inside a title, so a folder
whose name contains one shows the expansion.

## How the two find each other

AYDY writes its current port to
`~/Library/Application Support/app.aydy/listener.json` while it is listening, and
removes the file when it stops. The extension reads that file on every report, so a port
that moved is followed without anything being told about the move, and a file that is not
there means AYDY is not running.

The extension also binds a loopback port of its own and names it when it registers, which
is how AYDY reaches **this** window and no other. AYDY confirms that port with a `ping`
before it accepts the registration, and every answer this window gives names its own
process id: a closed window's port can be taken by something else, and that is the only
way to tell a stranger from the window that was asked.

Sending a command to the wrong window is not a cosmetic mistake. Asking a window to
reveal a session it does not host makes Claude Code open a second, live copy of that
session, which is why addressing is confirmed rather than assumed.

## What it depends on, and what it does not

It does **not** depend on the Claude Code extension's API, because there is none:
`Anthropic.claude-code` exports nothing from `activate`, so
`vscode.extensions.getExtension('Anthropic.claude-code').exports` is `undefined`. Every
version of this integration that assumed otherwise would be dead on arrival.

It does depend on one thing that is not an API: the arguments of
`claude-vscode.editor.open`, which is a public command id with an undocumented signature.
It was read out of the shipped bundle — `registerCommand("claude-vscode.editor.open",
async (sessionId, prompt, column) => …)` — verified against **2.1.220** during planning
and **2.1.238** while this was built. The bundle ships roughly weekly, so the reveal is
only attempted on the **2.1** line and anything else falls back to raising the window
without the tab. The version is reported to AYDY either way, so an unexpected one is a
fact in a bug report rather than a mystery. A window with no Claude Code extension at all
registers exactly the same way; it simply reveals nothing.

## Building it

From the repository root:

```sh
yarn build-extension
```

That compiles `src/` to `out/`, which `package.json` points `main` at. `yarn build-tsc`
type-checks it alongside the app, and `yarn test` runs its tests with the app's.

To produce the installable artifact:

```sh
yarn package-extension
```

That compiles `src/` and then runs `vsce package`, writing
`extension/aydy-companion-<version>.vsix`. `.vscodeignore` holds the sources, the tests
and the tsconfigs back; a VSIX carries the manifest, `out/` and this file, and the
`.vsixmanifest` inside it is written by `vsce` rather than by hand.

The manifest keeps `"private": true`. That is npm's guard against `npm publish`, and
`vsce` has no opinion about it, so packaging does not cost the guard. Nothing here is
published to the Marketplace either: `vsce publish` is not wired to any script, and it
would need a publisher account and a token that this repository does not carry.
