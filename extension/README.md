# AYDY Companion

Tracks Claude Code sessions for **AYDY** and shows how many need you in each window's
tab.

AYDY lists every active Claude Code session in the macOS menu bar. This extension feeds
it every session's state through files a hook script writes, keeps each window's count of
sessions that need you, and answers when AYDY asks to jump to a session.

## Installing it

Package it first — see [Building it](#building-it) — and then hand the artifact to VS
Code, from the repository root:

```sh
code --install-extension extension/aydy-companion-0.1.0.vsix
```

Uninstall the same way, so that the record of the install is unwound as well as the
files. Uninstalling also removes AYDY's hook entries from Claude Code's settings:

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
settings on purpose. It works whether or not AYDY is running, because it only reads and
writes files.

## What it does

Everything goes through files under `~/Library/Application Support/app.aydy/`:

- **`status/<session>.json`.** On activation the extension copies its hook script to
  `bin/aydy-hook.sh` there and adds it to the hooks in Claude Code's `settings.json`. Claude Code runs it on each hook event, and it writes the
  event as that session's status file, removing it when the session ends.
- **`seen/<session>.json`.** Each window writes one when you are looking at that
  window while one of its sessions has something new, which is what clears it.
- **`windows/<pid>.json`.** Each window publishes its workspace folders under its
  extension host's process id, and withdraws the file when it closes. A session belongs
  to the window whose folder most specifically contains the directory it was launched in.

The tab count and jump answers are described in their own sections below.

## Revealing a tab

The one thing AYDY asks this window to **do**: bring the tab that owns a session to the
front, so that a window with three sessions in it comes up showing the right one.

There is no supported way to do it. `vscode.TabGroups` can close a tab and nothing else,
and the Claude Code extension exports no API, so the only way in is the command it
contributes — `claude-vscode.editor.open`, called the way Claude's own webview calls it.
Given a session this window already has open, that command reveals the existing panel.
**Given one it does not, the same call creates a panel and resumes the session in it** —
a second, live Claude session running against the user's work, started silently.

So a reveal is refused unless every one of these holds, and the refusal is written as the
`reveal` field of the reply file rather than swallowed:

- **This window owns the session.** Only the window whose folder most specifically
  contains the session's launch directory claims the jump request.
- **A tab here carries the label the request names.** The label goes in the jump request
  file, and this window's `revealSession` matches it against its own Claude tabs. A
  session hosted in the sidebar is a view rather than a tab and never matches —
  correctly, since it has no tab to reveal.
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

## How a jump arrives

The app writes `jump/<id>.json`, and only the window that owns the session claims it, by
unlinking it. That ownership rule is what keeps a reveal from reaching a window that does
not host the session — and asking a window to reveal a session it does not host makes
Claude Code open a second, live copy of it.

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
without the tab. The version is only checked locally. A window with no Claude Code
extension at all still tracks sessions and answers jumps; it simply reveals nothing.

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
